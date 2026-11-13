import test from "node:test";
import assert from "node:assert/strict";
import { assessCooldown, cooldownMessage, utcDayStartMs } from "../lib/cooldown.js";
import { mergeRecent, parseIndexerAccount, parseIndexerTxns } from "../lib/recent.js";

const DAY = 86400000;

test("utc day start is midnight UTC", () => {
  const now = Date.UTC(2026, 10, 12, 18, 30, 0);
  assert.equal(utcDayStartMs(now), Date.UTC(2026, 10, 12));
});

test("a recent signature is refused until the cooldown elapses", () => {
  const now = Date.UTC(2026, 10, 12, 12, 0, 10);
  const decision = assessCooldown({
    nowMs: now,
    timestampsMs: [now - 5000],
    cooldownMs: 20000,
    maxPerDay: 40,
  });
  assert.equal(decision.allowed, false);
  assert.equal(decision.code, "cooldown");
  assert.equal(decision.retryAfterSeconds, 15);
  assert.match(cooldownMessage(decision, 20, 40), /once every 20 seconds/);
});

test("an old signature does not keep the cooldown closed", () => {
  const now = Date.UTC(2026, 10, 12, 12, 0, 30);
  const decision = assessCooldown({
    nowMs: now,
    timestampsMs: [now - 21000],
    cooldownMs: 20000,
    maxPerDay: 40,
  });
  assert.equal(decision.allowed, true);
  assert.equal(decision.attemptsToday, 1);
});

test("the daily attempt cap refuses even after the cooldown", () => {
  const start = Date.UTC(2026, 10, 12);
  const stamps = Array.from({ length: 40 }, (_, index) => start + index * 1000);
  const decision = assessCooldown({
    nowMs: start + DAY - 1000,
    timestampsMs: stamps,
    cooldownMs: 20000,
    maxPerDay: 40,
  });
  assert.equal(decision.allowed, false);
  assert.equal(decision.code, "daily_attempt_cap");
  assert.match(cooldownMessage(decision, 20, 40), /40 signed attempts/);
});

test("a generous daily cap still allows a judge's handful of attempts", () => {
  const start = Date.UTC(2026, 10, 13);
  const stamps = Array.from({ length: 40 }, (_, index) => start + index * 1000);
  const decision = assessCooldown({
    nowMs: start + 50_000,
    timestampsMs: stamps,
    cooldownMs: 0,
    maxPerDay: 300,
  });
  assert.equal(decision.allowed, true);
  assert.equal(decision.attemptsToday, 40);
});

test("yesterday's attempts do not count against today's cap", () => {
  const start = Date.UTC(2026, 10, 12);
  const decision = assessCooldown({
    nowMs: start + 1000,
    timestampsMs: [start - 5000, start - 4000],
    cooldownMs: 0,
    maxPerDay: 1,
  });
  assert.equal(decision.allowed, true);
  assert.equal(decision.attemptsToday, 0);
});

test("indexer account age uses the creation timestamp", () => {
  const parsed = parseIndexerAccount({
    data: { created: { block_timestamp: "1700000000000000000", transaction_hash: "abc" } },
  });
  assert.equal(parsed.createdAtMs, 1700000000000);
  assert.equal(parsed.creationHash, "abc");
  assert.equal(parseIndexerAccount({ data: {} }), null);
});

test("indexer transactions keep hash, time, and status", () => {
  const rows = parseIndexerTxns({
    data: [
      {
        transaction_hash: "hash1",
        signer_account_id: "agent.testnet",
        receiver_account_id: "wallet.testnet",
        block_timestamp: "1700000000000000000",
        outcomes: { status: false },
        actions: [{ method: "pay" }],
      },
      { transaction_hash: "", outcomes: { status: true }, actions: [] },
    ],
  });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].txHash, "hash1");
  assert.equal(rows[0].success, false);
  assert.deepEqual(rows[0].methods, ["pay"]);
  assert.equal(rows[0].atMs, 1700000000000);
});

test("journal details win over the index, and both keep explorer links", () => {
  const merged = mergeRecent(
    [{ txHash: "hash1", atMs: 10, success: false, methods: ["pay"], signer: "a", receiver: "b" }],
    [{ txHash: "hash1", atMs: 10, outcome: "failure", errorCode: "ERR_PAYEE_NOT_ALLOWED", title: "Prompt injection", riskVerdict: "block", kind: "attack" }],
    "https://testnet.nearblocks.io/txns/",
  );
  assert.equal(merged.length, 1);
  assert.equal(merged[0].errorCode, "ERR_PAYEE_NOT_ALLOWED");
  assert.equal(merged[0].riskVerdict, "block");
  assert.equal(merged[0].title, "Prompt injection");
  assert.equal(merged[0].explorerUrl, "https://testnet.nearblocks.io/txns/hash1");
});
