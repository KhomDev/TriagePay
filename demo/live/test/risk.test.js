import test from "node:test";
import assert from "node:assert/strict";
import { classifyAmount, classifyPayee, EMPTY_CODE_HASH } from "../lib/risk.js";

const NOW = Date.UTC(2026, 10, 12, 12, 0, 0);
const DAY = 86400000;
const WINDOW = 7 * DAY;
const OLD = NOW - 30 * DAY;
const YOUNG = NOW - DAY;
const HASH = "3tj0GNdgHy4pTqn5cH7nQh4p1xYk8mJw2sVb6cZd9aQe";
const OTHER = "9kQp2mN4xR7vL1sT8wYc5bH3jF6dA0eZuCqWnM8rP2oB";

function facts(overrides) {
  return {
    accountId: "payee.testnet",
    exists: true,
    codeHash: EMPTY_CODE_HASH,
    createdAtMs: OLD,
    nowMs: NOW,
    allowlist: ["payee.testnet"],
    flagged: ["flagged.testnet"],
    verifiedCodeHashes: [HASH],
    newAccountWindowMs: WINDOW,
    ...overrides,
  };
}

test("an old allowlisted account with no contract is allow", () => {
  const result = classifyPayee(facts({}));
  assert.equal(result.verdict, "allow");
  assert.deepEqual(result.reasons.map((item) => item.code), ["allowlisted"]);
});

test("a newly created allowlisted account is warn", () => {
  const result = classifyPayee(facts({ createdAtMs: YOUNG }));
  assert.equal(result.verdict, "warn");
  assert.equal(result.reasons[0].code, "newly_created");
  assert.match(result.reasons[0].text, /^newly created account/);
});

test("an account created exactly at the window boundary is not new", () => {
  const result = classifyPayee(facts({ createdAtMs: NOW - WINDOW }));
  assert.equal(result.verdict, "allow");
});

test("a missing creation time is warn, not a silent allow", () => {
  const result = classifyPayee(facts({ createdAtMs: null }));
  assert.equal(result.verdict, "warn");
  assert.equal(result.reasons[0].code, "age_unknown");
  assert.match(result.reasons[0].text, /age could not be confirmed/);
});

test("an account that is not on the allowlist is block", () => {
  const result = classifyPayee(facts({ accountId: "stranger.testnet", allowlist: ["payee.testnet"] }));
  assert.equal(result.verdict, "block");
  assert.ok(result.reasons.some((item) => item.code === "not_allowlisted"));
  assert.equal(result.reasons.find((item) => item.code === "not_allowlisted").text, "not on the payee allowlist");
});

test("a flagged account is block even when other reasons also apply", () => {
  const result = classifyPayee(facts({
    accountId: "flagged.testnet",
    createdAtMs: YOUNG,
    allowlist: ["payee.testnet"],
  }));
  assert.equal(result.verdict, "block");
  assert.deepEqual(result.reasons.map((item) => item.code), ["flagged", "not_allowlisted", "newly_created"]);
  assert.equal(result.reasons[0].text, "account is on the flagged list");
});

test("a missing account is block and does not claim a contract", () => {
  const result = classifyPayee(facts({
    accountId: "injected.testnet",
    exists: false,
    codeHash: null,
    createdAtMs: null,
    allowlist: ["payee.testnet"],
  }));
  assert.equal(result.verdict, "block");
  assert.deepEqual(result.reasons.map((item) => item.code), ["account_missing", "not_allowlisted"]);
  assert.equal(result.reasons[0].text, "account does not exist");
});

test("unknown contract code is block", () => {
  const result = classifyPayee(facts({
    accountId: "token.testnet",
    codeHash: OTHER,
    allowlist: ["payee.testnet"],
  }));
  assert.equal(result.verdict, "block");
  assert.ok(result.reasons.some((item) => item.code === "unverified_contract"));
  assert.equal(
    result.reasons.find((item) => item.code === "unverified_contract").text,
    "unknown or unverified contract",
  );
});

test("verified contract code on an allowlisted old account is allow", () => {
  const result = classifyPayee(facts({ codeHash: HASH }));
  assert.equal(result.verdict, "allow");
});

test("the empty code hash is not treated as a contract", () => {
  const result = classifyPayee(facts({ codeHash: EMPTY_CODE_HASH }));
  assert.equal(result.verdict, "allow");
});

test("block outranks warn", () => {
  const result = classifyPayee(facts({
    accountId: "flagged.testnet",
    createdAtMs: YOUNG,
    codeHash: OTHER,
  }));
  assert.equal(result.verdict, "block");
  assert.ok(result.reasons.some((item) => item.severity === "warn"));
  assert.ok(result.reasons.some((item) => item.severity === "block"));
});

test("one atomic unit over the per-call max is block", () => {
  const result = classifyAmount({ amount: "1001", perCallMax: "1000", spentToday: "0", dailyMax: "5000" });
  assert.equal(result.verdict, "block");
  assert.equal(result.reasons[0].code, "per_call_limit");
  assert.equal(result.reasons[0].text, "amount is above the per-call limit");
});

test("a payment that would cross the daily limit is block", () => {
  const result = classifyAmount({ amount: "100", perCallMax: "1000", spentToday: "4950", dailyMax: "5000" });
  assert.equal(result.verdict, "block");
  assert.ok(result.reasons.some((item) => item.code === "daily_limit"));
});

test("an in-limit payment is allow", () => {
  const result = classifyAmount({ amount: "100", perCallMax: "1000", spentToday: "0", dailyMax: "5000" });
  assert.equal(result.verdict, "allow");
  assert.equal(result.reasons[0].code, "within_limit");
});
