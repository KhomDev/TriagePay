import test from "node:test";
import assert from "node:assert/strict";
import { memoryStore } from "../lib/store.js";
import {
  clientIp,
  releaseLock,
  reserveAttempt,
  resetLocalLimits,
  takeLocalIp,
} from "../lib/limits.js";

const NOW = Date.UTC(2026, 10, 12, 12, 0, 0);

function chain(overrides = {}) {
  return {
    allowed: true,
    code: null,
    retryAfterSeconds: 0,
    attemptsToday: 0,
    ...overrides,
  };
}

test("client ip prefers x-real-ip and otherwise the first forwarded address", () => {
  const real = new Request("http://demo.local/api/run", { headers: { "x-real-ip": "203.0.113.8", "x-forwarded-for": "198.51.100.2" } });
  assert.equal(clientIp(real), "203.0.113.8");
  const forwarded = new Request("http://demo.local/api/run", { headers: { "x-forwarded-for": "198.51.100.2, 10.0.0.1" } });
  assert.equal(clientIp(forwarded), "198.51.100.2");
  assert.equal(clientIp(null), "unknown");
});

test("a shared store stops the fifth request from one address", async () => {
  const store = memoryStore(() => NOW);
  for (let i = 0; i < 4; i++) {
    const decision = await reserveAttempt({
      store,
      nowMs: NOW + i,
      ip: "203.0.113.9",
      maxPerDay: 40,
      cooldownMs: 0,
      ipLimit: 4,
      ipWindowMs: 60000,
      chain: chain(),
      lockToken: `t${i}`,
    });
    assert.equal(decision.allowed, true, decision.code);
    await releaseLock(store, decision.lockToken);
  }
  const blocked = await reserveAttempt({
    store,
    nowMs: NOW + 10,
    ip: "203.0.113.9",
    maxPerDay: 40,
    cooldownMs: 0,
    ipLimit: 4,
    ipWindowMs: 60000,
    chain: chain(),
    lockToken: "t4",
  });
  assert.equal(blocked.allowed, false);
  assert.equal(blocked.code, "ip_rate_limit");
  const other = await reserveAttempt({
    store,
    nowMs: NOW + 11,
    ip: "203.0.113.10",
    maxPerDay: 40,
    cooldownMs: 0,
    ipLimit: 4,
    ipWindowMs: 60000,
    chain: chain(),
    lockToken: "other",
  });
  assert.equal(other.allowed, true);
  await releaseLock(store, other.lockToken);
});

test("the daily cap holds in the store even when the chain index is behind", async () => {
  const store = memoryStore(() => NOW);
  const first = await reserveAttempt({
    store,
    nowMs: NOW,
    ip: "203.0.113.11",
    maxPerDay: 1,
    cooldownMs: 0,
    chain: chain({ attemptsToday: 0 }),
    lockToken: "a",
  });
  assert.equal(first.allowed, true);
  await releaseLock(store, first.lockToken);
  const second = await reserveAttempt({
    store,
    nowMs: NOW + 5,
    ip: "198.51.100.20",
    maxPerDay: 1,
    cooldownMs: 0,
    chain: chain({ attemptsToday: 0 }),
    lockToken: "b",
  });
  assert.equal(second.allowed, false);
  assert.equal(second.code, "daily_attempt_cap");
});

test("a chain daily cap refuses before the store is touched", async () => {
  let calls = 0;
  const store = { async incr() { calls += 1; return 1; } };
  const decision = await reserveAttempt({
    store,
    nowMs: NOW,
    ip: "203.0.113.12",
    maxPerDay: 40,
    cooldownMs: 20000,
    chain: chain({ allowed: false, code: "daily_attempt_cap", retryAfterSeconds: 9, attemptsToday: 40 }),
  });
  assert.equal(decision.allowed, false);
  assert.equal(decision.code, "daily_attempt_cap");
  assert.equal(calls, 0);
});

test("without a store, a missing index fails closed and a local address cap still applies", async () => {
  resetLocalLimits();
  const missing = await reserveAttempt({
    store: null,
    nowMs: NOW,
    ip: "203.0.113.13",
    maxPerDay: 40,
    cooldownMs: 0,
    chain: null,
  });
  assert.equal(missing.allowed, false);
  assert.equal(missing.code, "limit_unavailable");
  for (let i = 0; i < 4; i++) {
    const ok = await reserveAttempt({
      store: null,
      nowMs: NOW + i,
      ip: "203.0.113.14",
      maxPerDay: 40,
      cooldownMs: 0,
      ipLimit: 4,
      ipWindowMs: 60000,
      chain: chain(),
    });
    assert.equal(ok.allowed, true);
  }
  const blocked = await reserveAttempt({
    store: null,
    nowMs: NOW + 10,
    ip: "203.0.113.14",
    maxPerDay: 40,
    cooldownMs: 0,
    ipLimit: 4,
    ipWindowMs: 60000,
    chain: chain(),
  });
  assert.equal(blocked.code, "ip_rate_limit");
  assert.equal(blocked.source, "instance");
  assert.equal(takeLocalIp("203.0.113.15", NOW, 1, 60000).allowed, true);
  assert.equal(takeLocalIp("203.0.113.15", NOW + 1, 1, 60000).allowed, false);
});

test("two callers cannot hold the signing lock at once", async () => {
  const store = memoryStore(() => NOW);
  const first = await reserveAttempt({
    store,
    nowMs: NOW,
    ip: "203.0.113.16",
    maxPerDay: 40,
    cooldownMs: 0,
    chain: chain(),
    lockToken: "lock-a",
  });
  assert.equal(first.allowed, true);
  const second = await reserveAttempt({
    store,
    nowMs: NOW + 1,
    ip: "203.0.113.17",
    maxPerDay: 40,
    cooldownMs: 0,
    chain: chain(),
    lockToken: "lock-b",
  });
  assert.equal(second.allowed, false);
  assert.equal(second.code, "cooldown");
  await releaseLock(store, "lock-a");
  const third = await reserveAttempt({
    store,
    nowMs: NOW + 2,
    ip: "203.0.113.17",
    maxPerDay: 40,
    cooldownMs: 0,
    chain: chain({ attemptsToday: 1 }),
    lockToken: "lock-c",
  });
  assert.equal(third.allowed, true);
  await releaseLock(store, third.lockToken);
});
