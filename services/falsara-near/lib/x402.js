// x402 on NEAR for Falsara: the requirement, its memo, and the headers.
//
// The memo is the whole state. It is an HMAC over the route, the expiry, a
// random nonce, the hash of the exact finding, the price, the payee, the
// asset and the network, so the server needs no database:
//
// - a payment whose memo verifies for this body pays for this body;
// - the same proof and the same body get the same verdict again (the gates
//   are deterministic), which is the idempotent replay;
// - the same proof with any other body fails the HMAC and is refused.
//
// Header names and error reasons follow docs/protocols/x402-near.md.

import { createHash, createHmac, randomBytes, timingSafeEqual } from "node:crypto";

export const HEADER_REQUIRED = "payment-required";
export const HEADER_SIGNATURE = "payment-signature";
export const HEADER_RESPONSE = "payment-response";
export const MEMO_PREFIX = "fal1";
const MEMO = /^fal1_(\d{10})_([0-9a-f]{24})_([0-9a-f]{32})$/;

/** Canonical JSON: object keys sorted at every level. */
export function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

export function bodyHash(body) {
  return createHash("sha256").update(canonical(body)).digest("hex");
}

function mac(secret, fields) {
  return createHmac("sha256", secret).update(fields.join("|")).digest("hex").slice(0, 32);
}

function memoFields(terms, expiresAt, nonce, hash) {
  return [terms.route, String(expiresAt), nonce, hash, terms.amount, terms.payTo, terms.asset, terms.network];
}

/** A new memo for this body. `nowSec` and `nonce` are injectable for tests. */
export function issueMemo(secret, terms, body, nowSec = Math.floor(Date.now() / 1000), nonce = randomBytes(12).toString("hex")) {
  const expiresAt = nowSec + terms.maxTimeoutSeconds;
  const value = `${MEMO_PREFIX}_${expiresAt}_${nonce}_${mac(secret, memoFields(terms, expiresAt, nonce, bodyHash(body)))}`;
  return { memo: value, expiresAt };
}

/**
 * Check a memo against this body and these terms.
 * Returns { ok: true, expiresAt } or { ok: false, reason }.
 */
export function checkMemo(secret, terms, body, memo) {
  const match = MEMO.exec(String(memo || ""));
  if (!match) return { ok: false, reason: "invalid_payload" };
  const expiresAt = Number(match[1]);
  const expected = mac(secret, memoFields(terms, expiresAt, match[2], bodyHash(body)));
  const same = expected.length === match[3].length
    && timingSafeEqual(Buffer.from(expected), Buffer.from(match[3]));
  if (!same) return { ok: false, reason: "proof_already_consumed" };
  return { ok: true, expiresAt };
}

/** The PAYMENT-REQUIRED body for one route and one finding. */
export function requirement(terms, memo, expiresAt, resourceUrl) {
  return {
    x402Version: 2,
    resource: {
      url: resourceUrl,
      description: `Falsara ${terms.label}`,
      mimeType: "application/json",
      serviceName: "Falsara",
    },
    accepts: [
      {
        scheme: "exact",
        network: terms.network,
        amount: terms.amount,
        asset: terms.asset,
        payTo: terms.payTo,
        maxTimeoutSeconds: terms.maxTimeoutSeconds,
        extra: {
          assetTransferMethod: "near-tx-proof",
          paymentFlow: "upfront",
          memo,
          intentId: memo,
          expiresAt,
          sku: terms.sku,
          priceUsdc: terms.priceUsdc,
        },
      },
    ],
  };
}

export function encodeHeader(value) {
  return Buffer.from(JSON.stringify(value)).toString("base64");
}

/** Decode a base64 JSON header, or null. */
export function decodeHeader(raw) {
  if (!raw || raw.length > 8192) return null;
  try {
    const value = JSON.parse(Buffer.from(String(raw), "base64").toString("utf8"));
    return value && typeof value === "object" && !Array.isArray(value) ? value : null;
  } catch {
    return null;
  }
}

const HASH = /^[1-9A-HJ-NP-Za-km-z]{43,44}$/;
const ACCOUNT = /^(?=.{2,64}$)[a-z0-9]+([-_.][a-z0-9]+)*$/;

/**
 * Read a PAYMENT-SIGNATURE: the accepted terms must be ours, and the
 * payload must name a transaction and its signer.
 */
export function parseSignature(terms, decoded) {
  if (!decoded) return { ok: false, reason: "invalid_payload" };
  const accepted = decoded.accepted || {};
  const payload = decoded.payload || {};
  const memo = payload.intent_id || (accepted.extra && accepted.extra.memo);
  if (accepted.network !== terms.network || payload.network !== terms.network) return { ok: false, reason: "invalid_network" };
  if (accepted.asset !== terms.asset || accepted.payTo !== terms.payTo || String(accepted.amount) !== terms.amount) {
    return { ok: false, reason: "invalid_payload" };
  }
  if (!HASH.test(String(payload.tx_hash || ""))) return { ok: false, reason: "invalid_payload" };
  if (!ACCOUNT.test(String(payload.signer_id || ""))) return { ok: false, reason: "invalid_payload" };
  return { ok: true, memo: String(memo || ""), txHash: String(payload.tx_hash), signerId: String(payload.signer_id) };
}
