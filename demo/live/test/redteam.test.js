// Regression tests for the red-team review of 2026-11-13 (hosted demo).

import { test } from "node:test";
import assert from "node:assert/strict";

import { assertPublicApproval, restorePolicy } from "../lib/demo.js";
import { assertSameOrigin, readJson } from "../lib/http.js";
import { assertModelBudget, ownerAuthorized, resetModelBudget } from "../lib/guard.js";
import { changesDigest, outOfBounds } from "../lib/policy-nl.js";
import { GET as cron } from "../api/cron.js";

const config = {
  policy: {
    perCallMax: "1000",
    dailyMax: "20000",
    payees: ["payee.testnet"],
  },
};
const TOKEN = "t".repeat(32);

function req(headers = {}, url = "https://demo.example/api/review") {
  return new Request(url, { method: "POST", headers });
}

test("RT-02: a visitor cannot approve a held payment to an unknown payee", () => {
  const attacker = { payment_id: "7", receiver_id: "attacker.testnet", amount: "500" };
  assert.throws(() => assertPublicApproval(config, attacker, req(), {}), { code: "owner_only" });
  const tooBig = { payment_id: "8", receiver_id: "payee.testnet", amount: "1001" };
  assert.throws(() => assertPublicApproval(config, tooBig, req(), {}), { code: "owner_only" });
  assert.throws(() => assertPublicApproval(config, null, req(), {}), { code: "not_pending" });
  const fine = { payment_id: "9", receiver_id: "payee.testnet", amount: "1000" };
  assert.doesNotThrow(() => assertPublicApproval(config, fine, req(), {}));
  const owner = req({ authorization: `Bearer ${TOKEN}` });
  assert.doesNotThrow(() => assertPublicApproval(config, attacker, owner, { TRIAGEPAY_OWNER_ACTION_TOKEN: TOKEN }));
});

test("RT-04: restore keeps an owner's tighter chain policy", () => {
  const tightened = { active: false, per_call_max: "10", daily_max: "50", payees: ["payee.testnet"] };
  assert.deepEqual(restorePolicy(config, tightened), {
    perCallMax: "10",
    dailyMax: "50",
    payees: ["payee.testnet"],
  });
  const loosened = { active: false, per_call_max: "999999", daily_max: "999999", payees: ["payee.testnet", "attacker.testnet"] };
  assert.deepEqual(restorePolicy(config, loosened), {
    perCallMax: "1000",
    dailyMax: "20000",
    payees: ["payee.testnet"],
  });
  assert.throws(() => restorePolicy(config, { per_call_max: "1", daily_max: "1", payees: ["x.testnet"] }), {
    code: "restore_refused",
  });
});

test("RT-11: owner-signing POSTs must be same-origin JSON", async () => {
  assert.throws(() => assertSameOrigin(req({ origin: "https://evil.example" })), { code: "cross_origin" });
  assert.throws(() => assertSameOrigin(req({ "sec-fetch-site": "cross-site" })), { code: "cross_origin" });
  assert.throws(() => assertSameOrigin(req({ origin: "null" })), { code: "cross_origin" });
  assert.doesNotThrow(() => assertSameOrigin(req({ origin: "https://demo.example" })));
  assert.doesNotThrow(() => assertSameOrigin(req({ "sec-fetch-site": "same-origin" })));
  const form = new Request("https://demo.example/api/review", {
    method: "POST",
    headers: { "content-type": "text/plain" },
    body: JSON.stringify({ action: "approve", payment_id: "1" }),
  });
  await assert.rejects(readJson(form), { code: "bad_content_type" });
  const big = new Request("https://demo.example/api/review", {
    method: "POST",
    headers: { "content-type": "application/json", "content-length": "999999" },
    body: "{}",
  });
  await assert.rejects(readJson(big), { code: "body_too_large" });
});

test("RT-03: apply is bounded and bound to the previewed change set", () => {
  assert.equal(outOfBounds(config, { perCallMax: "10", dailyMax: "20", payees: ["payee.testnet"] }), null);
  assert.match(outOfBounds(config, { perCallMax: "10", dailyMax: "20", payees: ["attacker.testnet"] }), /allowlist/);
  assert.match(outOfBounds(config, { perCallMax: "10", dailyMax: "20001", payees: [] }), /daily/);
  const a = changesDigest([{ field: "daily max", from: "1", to: "2" }], { dailyMaxAtomic: "2" });
  const b = changesDigest([{ field: "daily max", from: "1", to: "3" }], { dailyMaxAtomic: "3" });
  assert.notEqual(a, b);
  assert.equal(a, changesDigest([{ field: "daily max", from: "1", to: "2" }], { dailyMaxAtomic: "2" }));
  assert.equal(ownerAuthorized(req({ authorization: `Bearer ${TOKEN}` }), {}), false);
  assert.equal(ownerAuthorized(req({ authorization: "Bearer wrong" }), { TRIAGEPAY_OWNER_ACTION_TOKEN: TOKEN }), false);
});

test("RT-05: model calls are counted before they are made", async () => {
  resetModelBudget();
  const env = { TRIAGEPAY_MODEL_CALLS_PER_IP: "2", TRIAGEPAY_MODEL_CALLS_PER_DAY: "3" };
  const from = (ip) => new Request("https://demo.example/api/policy", { headers: { "x-triagepay-peer": ip } });
  await assertModelBudget(from("a"), env, 1000);
  await assertModelBudget(from("a"), env, 1001);
  await assert.rejects(assertModelBudget(from("a"), env, 1002), { code: "model_rate_limit" });
  await assertModelBudget(from("b"), env, 1003);
  await assert.rejects(assertModelBudget(from("c"), env, 1004), { code: "model_daily_cap" });
  resetModelBudget();
});

test("L-3: the cron route signs nothing without CRON_SECRET", async () => {
  const saved = process.env.CRON_SECRET;
  delete process.env.CRON_SECRET;
  try {
    const res = await cron(new Request("https://demo.example/api/cron"));
    assert.equal(res.status, 503);
    process.env.CRON_SECRET = "cron-test-secret";
    const wrong = await cron(new Request("https://demo.example/api/cron", { headers: { authorization: "Bearer nope" } }));
    assert.equal(wrong.status, 401);
  } finally {
    if (saved == null) delete process.env.CRON_SECRET;
    else process.env.CRON_SECRET = saved;
  }
});

test("RT-06: the allowance floor parses NEAR amounts", async () => {
  const { allowanceFloor } = await import("../lib/demo.js");
  assert.equal(allowanceFloor({}), 50000000000000000000000n);
  assert.equal(allowanceFloor({ TRIAGEPAY_ALLOWANCE_FLOOR_NEAR: "0.1" }), 100000000000000000000000n);
  assert.equal(allowanceFloor({ TRIAGEPAY_ALLOWANCE_FLOOR_NEAR: "junk" }), 50000000000000000000000n);
});

test("stale holds: yesterday's and all but the newest two are cleared", async () => {
  const { staleHolds } = await import("../lib/demo.js");
  const now = Date.UTC(2026, 10, 14, 12);
  const today = String(Math.floor(now / 86_400_000));
  const yesterday = String(Number(today) - 1);
  const rows = [
    { payment_id: "20", window_index: yesterday },
    { payment_id: "30", window_index: today },
    { payment_id: "31", window_index: today },
    { payment_id: "32", window_index: today },
  ];
  assert.deepEqual(staleHolds(rows, now).sort(), ["20", "30"]);
  assert.deepEqual(staleHolds([{ payment_id: "5", window_index: today }], now), []);
  assert.deepEqual(staleHolds([], now), []);
});

test("the ticker says why, in words, and escapes chain text", async () => {
  const { tapeHtml, tapeWords } = await import("../lib/tape.js");
  const refused = tapeWords({ hash: "A".repeat(44), network: "testnet", paid: false, code: "ERR_PAYEE_NOT_ALLOWED", payee: "attacker.testnet" });
  assert.equal(refused.reason, "Payee not on the allowlist");
  assert.equal(refused.payee, "attacker.testnet");
  const html = tapeHtml([{ hash: "B".repeat(44), network: "mainnet", paid: false, code: "<b>", payee: "<script>" }]);
  assert.equal(/<script>|<b>/.test(html), false);
  assert.match(html, /Refused by the contract/);
  assert.match(tapeHtml([{ hash: "C".repeat(44), network: "mainnet", paid: true }]), /Allowlisted payee, inside the caps/);
});

test("Falsara: the agent checks the requirement before it pays", async () => {
  const { acceptableRequirement, USDC_MAINNET } = await import("../lib/falsara.js");
  const good = {
    scheme: "exact",
    network: "near:mainnet",
    asset: USDC_MAINNET,
    payTo: "gateway.hlpok1lad.tg",
    amount: "250000",
    extra: { memo: "fal1_1794624471_f3740123456789abcdef0123_0123456789abcdef0123456789abcdef" },
  };
  const wallet = { payees: ["gateway.hlpok1lad.tg"], perCallMax: "250000" };
  assert.equal(acceptableRequirement(good, wallet), null);
  assert.match(acceptableRequirement({ ...good, network: "near:testnet" }, wallet), /mainnet/);
  assert.match(acceptableRequirement({ ...good, asset: "wrap.near" }, wallet), /USDC/);
  assert.match(acceptableRequirement({ ...good, payTo: "attacker.near" }, wallet), /allowlist/);
  assert.match(acceptableRequirement({ ...good, amount: "250001" }, wallet), /per-call cap/);
  assert.match(acceptableRequirement({ ...good, extra: {} }, wallet), /memo/);
});

test("Falsara: an undecidable finding is reported without paying", async () => {
  const { buyFalsaraCheck } = await import("../lib/falsara.js");
  const fetchImpl = async () => new Response(JSON.stringify({ error: "INCOMPLETE_NOT_CHARGED", missing_evidence: ["G1"] }), { status: 422 });
  const result = await buyFalsaraCheck({ env: { TRIAGEPAY_MAINNET_AGENT_SECRET_KEY: "ed25519:" + "1".repeat(60), FALSARA_URL: "https://falsara.test" }, fetchImpl });
  assert.equal(result.charged, false);
  assert.equal(result.verdict, "INCOMPLETE");
});
