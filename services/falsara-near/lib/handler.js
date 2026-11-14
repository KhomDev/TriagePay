// The Falsara NEAR gateway. One handler for every route so the deploy is a
// single serverless function.
//
//   GET  /health, /pricing, /.well-known/x402
//   POST /v1/preflight                 free; says whether a check would decide
//   POST /v1/check-finding             x402 on NEAR, 0.25 USDC
//   POST /v1/check-finding-poc         x402 on NEAR, 0.75 USDC
//
// Pay only for a decision: before it asks for payment the gateway runs the
// gates once. A finding the gates cannot decide (INCOMPLETE) gets 422
// INCOMPLETE_NOT_CHARGED and no payment requirement.

import { ready } from "./config.js";
import { coreHealth, runCore } from "./core.js";
import {
  HEADER_REQUIRED,
  HEADER_RESPONSE,
  HEADER_SIGNATURE,
  checkMemo,
  decodeHeader,
  encodeHeader,
  issueMemo,
  parseSignature,
  requirement,
} from "./x402.js";
import { verifyPayment } from "./verify.js";

const BODY_LIMIT = 64 * 1024;
const UNPAID_PER_MINUTE = 12;
const hits = new Map();

function json(status, body, headers = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
      "access-control-allow-origin": "*",
      "access-control-expose-headers": "payment-required, payment-response",
      ...headers,
    },
  });
}

export function clientIp(request, env = process.env) {
  const header = (name) => {
    const value = request.headers.get(name);
    return value && value.trim() ? value.split(",")[0].trim() : null;
  };
  if (env.VERCEL === "1") return header("x-vercel-forwarded-for") || header("x-real-ip") || "unknown";
  return "local";
}

export function resetLimits() {
  hits.clear();
}

function limited(ip, nowMs) {
  const recent = (hits.get(ip) || []).filter((at) => nowMs - at < 60_000);
  if (recent.length >= UNPAID_PER_MINUTE) {
    hits.set(ip, recent);
    return true;
  }
  recent.push(nowMs);
  hits.set(ip, recent);
  return false;
}

async function readBody(request) {
  const declared = Number(request.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > BODY_LIMIT) return { error: json(413, { error: "BODY_TOO_LARGE" }) };
  const text = await request.text();
  if (text.length > BODY_LIMIT) return { error: json(413, { error: "BODY_TOO_LARGE" }) };
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return { error: json(400, { error: "INVALID_JSON" }) };
  }
  if (!body || typeof body !== "object" || Array.isArray(body)) return { error: json(400, { error: "INVALID_FINDING" }) };
  return { body };
}

function pricing(config) {
  return {
    service: "Falsara",
    network: config.network,
    asset: config.asset,
    assetSymbol: "USDC",
    payTo: config.payTo,
    routes: Object.values(config.routes).map((route) => ({
      method: "POST",
      path: route.route,
      priceUsdc: route.priceUsdc,
      amountAtomic: route.amount,
      sku: route.sku,
    })),
    charging: "Only decisions are charged. A finding the gates cannot decide gets 422 INCOMPLETE_NOT_CHARGED before any payment is asked.",
  };
}

function paymentFailure(reason, message, extraHeaders = {}) {
  return json(402, { error: reason, message }, {
    [HEADER_RESPONSE]: encodeHeader({ success: false, errorReason: reason }),
    ...extraHeaders,
  });
}

export async function handle(request, { config, fetchImpl = fetch, nowMs = Date.now(), env = process.env } = {}) {
  const url = new URL(request.url);
  const path = url.pathname.replace(/\/+$/, "") || "/";

  if (request.method === "OPTIONS") {
    return new Response(null, {
      status: 204,
      headers: {
        "access-control-allow-origin": "*",
        "access-control-allow-methods": "GET, POST, OPTIONS",
        "access-control-allow-headers": "content-type, payment-signature",
        "access-control-expose-headers": "payment-required, payment-response",
      },
    });
  }

  if (request.method === "GET") {
    if (path === "/health") {
      const core = await coreHealth(config, fetchImpl);
      // The core's pins are its own claim; healthy means they equal the
      // pins this gateway expects (ToB C-VA-6).
      const ok = core.ok && core.pinsMatch && ready(config);
      return json(ok ? 200 : 503, {
        ok,
        service: "falsara-near",
        network: config.network,
        payTo: config.payTo,
        core: {
          ok: core.ok,
          pins: core.pins || null,
          expectedPins: core.expectedPins,
          pinsMatch: core.pinsMatch,
          ...(core.drift && core.drift.length ? { drift: core.drift } : {}),
        },
      });
    }
    if (path === "/pricing") return json(200, pricing(config));
    if (path === "/.well-known/x402") {
      return json(200, {
        x402Version: 2,
        resources: Object.values(config.routes).map((route) => `${config.publicBaseUrl}${route.route}`),
        pricing: pricing(config),
      });
    }
    return json(404, { error: "NOT_FOUND" });
  }

  if (request.method !== "POST") return json(405, { error: "METHOD_NOT_ALLOWED" });
  if (!ready(config)) return json(503, { error: "NOT_CONFIGURED" });

  const preflight = path === "/v1/preflight";
  const route = preflight ? config.routes[url.searchParams.get("route") || "/v1/check-finding"] : config.routes[path];
  if (!route) return json(404, { error: "NOT_FOUND" });

  const read = await readBody(request);
  if (read.error) return read.error;
  const body = read.body;
  const signatureHeader = request.headers.get(HEADER_SIGNATURE);

  // Unpaid: decide whether this finding is decidable before asking for money.
  if (!signatureHeader) {
    if (limited(clientIp(request, env), nowMs)) {
      return json(429, { error: "RATE_LIMITED", message: "Too many unpaid checks from this address this minute." }, { "retry-after": "60" });
    }
    const core = await runCore(config, route, body, fetchImpl);
    if (core.status !== 200) return json(core.status, core.body);
    const decisive = core.body.verdict === "REFUSE" || core.body.verdict === "NO_REFUSAL";
    if (preflight) {
      return json(200, {
        decisive,
        charged: decisive,
        priceUsdc: route.priceUsdc,
        missing_evidence: decisive ? [] : core.body.missing_evidence || [],
      });
    }
    if (!decisive) {
      return json(422, {
        error: "INCOMPLETE_NOT_CHARGED",
        message: "The gates cannot decide this finding yet. Nothing was charged. Add the missing evidence and try again.",
        missing_evidence: core.body.missing_evidence || [],
      });
    }
    const { memo, expiresAt } = issueMemo(config.memoSecret, route, body, Math.floor(nowMs / 1000));
    const required = requirement(route, memo, expiresAt, `${config.publicBaseUrl}${route.route}`);
    return json(402, { ...required, error: "PAYMENT-SIGNATURE header is required" }, { [HEADER_REQUIRED]: encodeHeader(required) });
  }

  if (preflight) return json(400, { error: "PREFLIGHT_IS_FREE", message: "Send preflight without a payment." });

  // Paid: the proof must be for this body, on chain, final and in time.
  const signature = parseSignature(route, decodeHeader(signatureHeader));
  if (!signature.ok) return paymentFailure(signature.reason, "The payment signature does not match these terms.");
  const memo = checkMemo(config.memoSecret, route, body, signature.memo);
  if (!memo.ok) {
    return paymentFailure(memo.reason, memo.reason === "proof_already_consumed"
      ? "This payment was made for a different request. Request a fresh requirement for this finding."
      : "The memo is not one this server issued.");
  }
  const paid = await verifyPayment({
    endpoints: config.endpoints,
    terms: route,
    memo: signature.memo,
    txHash: signature.txHash,
    signerId: signature.signerId,
    expiresAt: memo.expiresAt,
    fetchImpl,
  });
  if (paid.status === "pending" || paid.status === "unavailable") {
    return paymentFailure("settlement_pending", "The transaction is not final yet, or the chain could not be read. Retry the same proof.", { "retry-after": "3" });
  }
  if (paid.status === "expired") return paymentFailure("requirement_expired", "The payment landed after the requirement expired.");
  if (paid.status === "failed") return paymentFailure("invalid_payload", "The transaction failed on chain.");
  if (paid.status !== "confirmed") return paymentFailure(paid.code || "invalid_payload", paid.reason || "The transfer does not match the requirement.");

  const core = await runCore(config, route, body, fetchImpl);
  if (core.status !== 200) {
    return json(503, {
      error: "CORE_UNAVAILABLE",
      message: "The payment is verified. The check did not run; retry the same request and proof.",
    });
  }
  const paymentResponse = {
    success: true,
    network: config.network,
    transaction: signature.txHash,
    payer: paid.payer,
    amount: paid.amount,
  };
  return json(200, {
    ...core.body,
    payment: {
      ...paymentResponse,
      asset: route.asset,
      payTo: route.payTo,
      memo: signature.memo,
      explorerUrl: config.explorerTxBase + signature.txHash,
    },
  }, { [HEADER_RESPONSE]: encodeHeader(paymentResponse) });
}
