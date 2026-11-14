import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { loadConfig, USDC_MAINNET } from "../lib/config.js";
import { handle, resetLimits } from "../lib/handler.js";
import { bodyHash, canonical, checkMemo, decodeHeader, encodeHeader, issueMemo } from "../lib/x402.js";
import { matchTransaction, nep141Transfers } from "../lib/verify.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURE = JSON.parse(readFileSync(join(HERE, "fixtures", "usdc-pay-CwLaJmaX.json"), "utf8")).result;
const FIXTURE_MEMO = "aps2_6af7a9d3_00003_7e6f99f0696491feba525c577ae29c84";
const SECRET = "s".repeat(40);
const NOW = Date.UTC(2026, 10, 14, 12, 0, 0);

const config = loadConfig({
  FALSARA_CORE_URL: "https://core.test",
  FALSARA_CORE_TOKEN: "t".repeat(40),
  FALSARA_MEMO_SECRET: SECRET,
  FALSARA_PUBLIC_BASE_URL: "https://api.falsara.online",
});
const route = config.routes["/v1/check-finding"];
const finding = {
  schema_version: "1.0.0",
  title: "Share price inflation",
  severity: "high",
  protocol: "ExampleVault",
  contract: "src/Vault.sol",
  impact: "An unprivileged attacker donates the underlying asset and takes 4 tokens from the next depositor.",
  preconditions: "The vault already has one depositor. The caller is not an admin.",
};

test("canonical JSON ignores key order", () => {
  assert.equal(canonical({ b: 1, a: [{ d: 2, c: 3 }] }), canonical({ a: [{ c: 3, d: 2 }], b: 1 }));
  assert.equal(bodyHash({ b: 1, a: 2 }), bodyHash({ a: 2, b: 1 }));
});

test("a memo pays for exactly one body under exactly these terms", () => {
  const { memo, expiresAt } = issueMemo(SECRET, route, finding, 1_800_000_000);
  assert.match(memo, /^fal1_\d{10}_[0-9a-f]{24}_[0-9a-f]{32}$/);
  assert.ok(memo.length <= 256);
  assert.deepEqual(checkMemo(SECRET, route, finding, memo), { ok: true, expiresAt });
  assert.equal(checkMemo(SECRET, route, { ...finding, title: "Other" }, memo).reason, "proof_already_consumed");
  assert.equal(checkMemo(SECRET, { ...route, amount: "1" }, finding, memo).reason, "proof_already_consumed");
  assert.equal(checkMemo("x".repeat(40), route, finding, memo).reason, "proof_already_consumed");
  assert.equal(checkMemo(SECRET, route, finding, "aps2_x").reason, "invalid_payload");
});

test("the verifier reads the real mainnet USDC receipt", () => {
  const terms = { asset: USDC_MAINNET, payTo: "gateway.hlpok1lad.tg", amount: "1000" };
  const ok = matchTransaction(FIXTURE, terms, FIXTURE_MEMO);
  assert.equal(ok.status, "confirmed");
  assert.equal(ok.payer, "e2e044e80.triagepay.hlpok1lad.tg");
  assert.equal(matchTransaction(FIXTURE, { ...terms, amount: "250000" }, FIXTURE_MEMO).status, "mismatch");
  assert.equal(matchTransaction(FIXTURE, terms, "fal1_other").status, "mismatch");
  assert.equal(matchTransaction(FIXTURE, { ...terms, payTo: "attacker.near" }, FIXTURE_MEMO).status, "mismatch");
  assert.equal(matchTransaction({ ...FIXTURE, final_execution_status: "INCLUDED" }, terms, FIXTURE_MEMO).status, "pending");
});

test("a perfect event logged by any other contract does not count", () => {
  const terms = { asset: USDC_MAINNET, payTo: "gateway.hlpok1lad.tg", amount: "1000" };
  const spoof = structuredClone(FIXTURE);
  for (const receipt of spoof.receipts_outcome) {
    if (receipt.outcome.executor_id === USDC_MAINNET) receipt.outcome.executor_id = "usdc-lookalike.near";
  }
  assert.equal(matchTransaction(spoof, terms, FIXTURE_MEMO).status, "mismatch");
  const failed = structuredClone(FIXTURE);
  for (const receipt of failed.receipts_outcome) {
    if (receipt.outcome.executor_id === USDC_MAINNET) receipt.outcome.status = { Failure: { ActionError: {} } };
  }
  assert.equal(matchTransaction(failed, terms, FIXTURE_MEMO).status, "mismatch");
  assert.equal(nep141Transfers(["EVENT_JSON:not json", "plain"]).length, 0);
});

// ---- the handler, with a fake core and a fake chain --------------------------------

function fakeFetch({ verdict = "NO_REFUSAL", chain = null, blockTimeSec = Math.floor(NOW / 1000) } = {}) {
  const calls = { core: 0 };
  async function impl(url, init = {}) {
    const href = String(url);
    if (href.startsWith("https://core.test")) {
      calls.core += 1;
      if (href.endsWith("/health")) return new Response(JSON.stringify({ status: "ok", pins: { "viability-gate": "0.7.0" } }));
      assert.equal(init.headers["x-ail-internal-token"], "t".repeat(40));
      return new Response(JSON.stringify({
        schema_version: "1.0.0",
        verdict,
        hits: [],
        missing_evidence: verdict === "INCOMPLETE" ? ["deployed_state"] : [],
        viability: {},
        realism: null,
      }));
    }
    const rpc = JSON.parse(init.body);
    if (rpc.method === "EXPERIMENTAL_tx_status") {
      if (!chain) return new Response(JSON.stringify({ jsonrpc: "2.0", error: { message: "doesn't exist", cause: { name: "UNKNOWN_TRANSACTION" } } }));
      return new Response(JSON.stringify({ jsonrpc: "2.0", result: chain }));
    }
    if (rpc.method === "block") {
      return new Response(JSON.stringify({ jsonrpc: "2.0", result: { header: { timestamp_nanosec: String(BigInt(blockTimeSec) * 1_000_000_000n) } } }));
    }
    throw new Error("unexpected fetch " + href);
  }
  impl.calls = calls;
  return impl;
}

function post(body, headers = {}) {
  return new Request("https://api.falsara.online/v1/check-finding", {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify(body),
  });
}

function paidChain(memo, amount = "250000") {
  const chain = structuredClone(FIXTURE);
  for (const receipt of chain.receipts_outcome) {
    receipt.outcome.logs = receipt.outcome.logs.map((line) => line.replace(FIXTURE_MEMO, memo).replace(/"amount":"1000"/g, `"amount":"${amount}"`));
  }
  return chain;
}

function signatureFor(required, memo, txHash = "CwLaJmaX8K2jYaTBvM1kyYfMumiUwFTfx49Pra5uPEgG") {
  return encodeHeader({
    x402Version: 2,
    accepted: required.accepts[0],
    payload: { network: "near:mainnet", intent_id: memo, tx_hash: txHash, signer_id: "tp-e2e-agent.hlpok1lad.tg" },
  });
}

test("an unpaid decisive check gets a NEAR USDC requirement bound to the finding", async () => {
  resetLimits();
  const res = await handle(post(finding), { config, fetchImpl: fakeFetch(), nowMs: NOW });
  assert.equal(res.status, 402);
  const required = decodeHeader(res.headers.get("payment-required"));
  const accepted = required.accepts[0];
  assert.equal(accepted.network, "near:mainnet");
  assert.equal(accepted.asset, USDC_MAINNET);
  assert.equal(accepted.amount, "250000");
  assert.equal(accepted.payTo, "gateway.hlpok1lad.tg");
  assert.equal(accepted.extra.assetTransferMethod, "near-tx-proof");
  assert.equal(checkMemo(SECRET, route, finding, accepted.extra.memo).ok, true);
});

test("an INCOMPLETE finding is never charged: 422 and no requirement", async () => {
  resetLimits();
  const res = await handle(post(finding), { config, fetchImpl: fakeFetch({ verdict: "INCOMPLETE" }), nowMs: NOW });
  assert.equal(res.status, 422);
  assert.equal(res.headers.get("payment-required"), null);
  const body = await res.json();
  assert.equal(body.error, "INCOMPLETE_NOT_CHARGED");
  assert.deepEqual(body.missing_evidence, ["deployed_state"]);
});

test("a paid check returns the verdict; replay repeats it; reuse for another finding is refused", async () => {
  resetLimits();
  const first = await handle(post(finding), { config, fetchImpl: fakeFetch(), nowMs: NOW });
  const required = decodeHeader(first.headers.get("payment-required"));
  const memo = required.accepts[0].extra.memo;
  const fetchImpl = fakeFetch({ chain: paidChain(memo) });
  const paid = await handle(post(finding, { "payment-signature": signatureFor(required, memo) }), { config, fetchImpl, nowMs: NOW });
  assert.equal(paid.status, 200);
  const body = await paid.json();
  assert.equal(body.verdict, "NO_REFUSAL");
  assert.equal(body.payment.payer, "e2e044e80.triagepay.hlpok1lad.tg");
  assert.equal(decodeHeader(paid.headers.get("payment-response")).success, true);

  const replay = await handle(post(finding, { "payment-signature": signatureFor(required, memo) }), { config, fetchImpl, nowMs: NOW });
  assert.equal(replay.status, 200);

  const reuse = await handle(post({ ...finding, title: "Another finding" }, { "payment-signature": signatureFor(required, memo) }), { config, fetchImpl, nowMs: NOW });
  assert.equal(reuse.status, 402);
  assert.equal((await reuse.json()).error, "proof_already_consumed");
});

test("pending, short, late and foreign-term payments are refused with the profile's reasons", async () => {
  resetLimits();
  const first = await handle(post(finding), { config, fetchImpl: fakeFetch(), nowMs: NOW });
  const required = decodeHeader(first.headers.get("payment-required"));
  const memo = required.accepts[0].extra.memo;
  const sig = { "payment-signature": signatureFor(required, memo) };

  const pending = await handle(post(finding, sig), { config, fetchImpl: fakeFetch({ chain: null }), nowMs: NOW });
  assert.equal((await pending.json()).error, "settlement_pending");

  const short = await handle(post(finding, sig), { config, fetchImpl: fakeFetch({ chain: paidChain(memo, "1000") }), nowMs: NOW });
  assert.equal((await short.json()).error, "amount_mismatch");

  const late = await handle(post(finding, sig), {
    config,
    fetchImpl: fakeFetch({ chain: paidChain(memo), blockTimeSec: Math.floor(NOW / 1000) + 3600 }),
    nowMs: NOW,
  });
  assert.equal((await late.json()).error, "requirement_expired");

  const cheaper = { ...required.accepts[0], amount: "1" };
  const forged = encodeHeader({ x402Version: 2, accepted: cheaper, payload: { network: "near:mainnet", intent_id: memo, tx_hash: "CwLaJmaX8K2jYaTBvM1kyYfMumiUwFTfx49Pra5uPEgG", signer_id: "tp-e2e-agent.hlpok1lad.tg" } });
  const terms = await handle(post(finding, { "payment-signature": forged }), { config, fetchImpl: fakeFetch(), nowMs: NOW });
  assert.equal((await terms.json()).error, "invalid_payload");
});

test("a proof whose memo or payee does not match is invalid_payload", async () => {
  resetLimits();
  const first = await handle(post(finding), { config, fetchImpl: fakeFetch(), nowMs: NOW });
  const required = decodeHeader(first.headers.get("payment-required"));
  const memo = required.accepts[0].extra.memo;
  const other = await handle(post(finding, { "payment-signature": signatureFor(required, memo) }), { config, fetchImpl: fakeFetch({ chain: FIXTURE }), nowMs: NOW });
  assert.equal((await other.json()).error, "invalid_payload");
});

test("preflight is free and says only whether a check would decide", async () => {
  resetLimits();
  const req = new Request("https://api.falsara.online/v1/preflight", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(finding) });
  const res = await handle(req, { config, fetchImpl: fakeFetch({ verdict: "REFUSE" }), nowMs: NOW });
  assert.equal(res.status, 200);
  const body = await res.json();
  assert.equal(body.decisive, true);
  assert.equal(body.verdict, undefined);
});

test("GET routes describe the service; unconfigured POSTs answer 503", async () => {
  const pricing = await handle(new Request("https://api.falsara.online/pricing"), { config, fetchImpl: fakeFetch() });
  const body = await pricing.json();
  assert.equal(body.routes[0].priceUsdc, "0.25");
  assert.equal(body.network, "near:mainnet");
  const health = await handle(new Request("https://api.falsara.online/health"), { config, fetchImpl: fakeFetch() });
  assert.equal(health.status, 200);
  const bare = loadConfig({});
  const res = await handle(post(finding), { config: bare, fetchImpl: fakeFetch() });
  assert.equal(res.status, 503);
});

test("unpaid checks are rate limited per address", async () => {
  resetLimits();
  let last;
  for (let i = 0; i < 13; i++) last = await handle(post(finding), { config, fetchImpl: fakeFetch(), nowMs: NOW + i });
  assert.equal(last.status, 429);
});
