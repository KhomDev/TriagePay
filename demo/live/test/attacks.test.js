import test from "node:test";
import assert from "node:assert/strict";
import { describeAttack, explain, listScenarios, shouldSubmit } from "../lib/attacks.js";

const config = {
  accounts: {
    payee: "payee.testnet",
    injected: "injected.testnet",
    flagged: "flagged.testnet",
  },
  policy: {
    perCallMax: "1000000000000000000000",
    dailyMax: "20000000000000000000000",
    allowedAmount: "100000000000000000000",
    decimals: 24,
    payees: ["payee.testnet"],
  },
  flaggedAccounts: ["flagged.testnet"],
};

test("the three attacks are selectable and distinct", () => {
  const scenarios = listScenarios(config);
  assert.deepEqual(scenarios.map((item) => item.id), [
    "injected-payee",
    "over-limit",
    "flagged-payee",
    "allowed",
    "review-payee",
    "above-threshold",
  ]);
});

test("prompt injection pays the hidden account, not the allowlisted payee", () => {
  const attack = describeAttack("injected-payee", config);
  assert.equal(attack.kind, "attack");
  assert.equal(attack.payee, "injected.testnet");
  assert.notEqual(attack.payee, config.accounts.payee);
  assert.equal(attack.expectedError, "ERR_PAYEE_NOT_ALLOWED");
  assert.match(attack.injection, /Ignore previous instructions/);
  assert.match(attack.injection, /injected\.testnet/);
  assert.equal(attack.amount, config.policy.allowedAmount);
});

test("over-limit injection is one atomic unit over the per-call cap", () => {
  const attack = describeAttack("over-limit", config);
  assert.equal(attack.payee, "payee.testnet");
  assert.equal(attack.amount, (BigInt(config.policy.perCallMax) + 1n).toString());
  assert.equal(attack.expectedError, "ERR_PER_CALL_LIMIT");
  assert.match(attack.injection, /per-call limit does not apply/);
  assert.equal(attack.amountDisplay, "0.001 wNEAR + 1 yocto");
});

test("flagged-payee injection names the flagged account", () => {
  const attack = describeAttack("flagged-payee", config);
  assert.equal(attack.payee, "flagged.testnet");
  assert.ok(config.flaggedAccounts.includes(attack.payee));
  assert.equal(attack.expectedError, "ERR_PAYEE_NOT_ALLOWED");
  assert.match(attack.injection, /skip the allowlist/);
  assert.notEqual(attack.payee, config.accounts.payee);
});

test("the allowed payment stays inside the per-call cap", () => {
  const attack = describeAttack("allowed", config);
  assert.equal(attack.kind, "payment");
  assert.equal(attack.payee, "payee.testnet");
  assert.equal(attack.expectedError, null);
  assert.ok(BigInt(attack.amount) < BigInt(config.policy.perCallMax));
  assert.equal(attack.injection, null);
});

test("unknown scenario ids are rejected", () => {
  assert.equal(describeAttack("nope", config), null);
});

test("attacks are still submitted when the risk check blocks", () => {
  const attack = describeAttack("injected-payee", config);
  const risk = { verdict: "block", reasons: [{ severity: "block", text: "not on the payee allowlist" }] };
  const amount = { verdict: "allow", reasons: [] };
  assert.equal(shouldSubmit(attack, risk, amount), true);
});

test("an allowed payment is not signed when the payee is block", () => {
  const attack = describeAttack("allowed", config);
  const risk = { verdict: "block", reasons: [{ severity: "block", text: "account is on the flagged list" }] };
  const amount = { verdict: "allow", reasons: [{ severity: "allow", text: "within the per-call and daily limits" }] };
  assert.equal(shouldSubmit(attack, risk, amount), false);
  const summary = explain(attack, risk, amount, { submitted: false });
  assert.match(summary, /Stopped before signing/);
  assert.match(summary, /flagged list/);
});

test("a refused chain attempt names the error and the risk reason", () => {
  const attack = describeAttack("over-limit", config);
  const risk = { verdict: "warn", reasons: [{ severity: "warn", text: "newly created account (under 7 days)" }] };
  const amount = { verdict: "block", reasons: [{ severity: "block", text: "amount is above the per-call limit" }] };
  const summary = explain(attack, risk, amount, {
    submitted: true,
    outcome: "failure",
    errorCode: "ERR_PER_CALL_LIMIT",
    rejected: false,
  });
  assert.match(summary, /ERR_PER_CALL_LIMIT/);
  assert.match(summary, /above the per-call limit/);
  assert.match(summary, /newly created account/);
});

test("a settled payment with no warning says the wallet settled it", () => {
  const attack = describeAttack("allowed", config);
  const risk = { verdict: "allow", reasons: [{ severity: "allow", text: "on the payee allowlist" }] };
  const amount = { verdict: "allow", reasons: [{ severity: "allow", text: "within the per-call and daily limits" }] };
  const summary = explain(attack, risk, amount, { submitted: true, outcome: "success", errorCode: null });
  assert.equal(summary, "The wallet settled the payment.");
});
