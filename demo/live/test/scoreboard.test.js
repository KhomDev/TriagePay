import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { GET as liveGet } from "../api/live.js";
import { fillHome, tapeFromRows } from "../lib/tape.js";
import { HOME_CACHE } from "../lib/live-board.js";
import { persistedAttempts, signedAttemptCount } from "../lib/board-store.js";
import {
  amountFromRpcStatus,
  amountFromTxn,
  presentScoreboard,
  publishAttempts,
  scoreboard,
  sumProtected,
  raiseScoreboard,
  takeAll,
  takeNewer,
  tallyPaySet,
} from "../lib/scoreboard.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function page(rows, next) {
  return { ok: true, rows, next: next || null };
}

const pay = (hash, success, atMs) => ({
  txHash: hash,
  signer: "tpd-e947e7-agent.testnet",
  methods: ["pay"],
  success,
  atMs,
});

test("the counted set dedupes by hash and ignores a non-pay transaction", () => {
  const rows = [
    pay("aaa", true, 3_000),
    pay("aaa", true, 3_000),
    { txHash: "bbb", signer: "other.testnet", methods: ["set_agent"], success: true, atMs: 2_000 },
    pay("ccc", false, 1_000),
  ];
  const tally = tallyPaySet(rows, 0);
  assert.equal(tally.paid, 1);
  assert.equal(tally.refused, 1);
  assert.deepEqual(tally.agentIds, ["tpd-e947e7-agent.testnet"]);
  assert.deepEqual(tally.refusedHashes, ["ccc"]);
  assert.equal(tally.attemptsToday, 3);
});

test("a partial index page is not a total", () => {
  const stopped = takeAll([page([pay("aaa", true, 2)], "cursor")], 80);
  assert.equal(stopped.complete, false);
  const finished = takeAll([page([pay("aaa", true, 2)], null)], 80);
  assert.equal(finished.complete, true);
  assert.equal(finished.rows.length, 1);
});

test("live rows stop at the snapshot watermark and do not recount it", () => {
  const watermark = { newestHash: "old", newestAtMs: 1_000 };
  const hit = takeNewer([
    page([pay("new", false, 2_000), pay("old", true, 1_000)], "more"),
  ], watermark);
  assert.equal(hit.ok, true);
  assert.deepEqual(hit.rows.map((row) => row.txHash), ["new"]);
  const missed = takeNewer([
    page([pay("new", false, 2_000)], null),
  ], watermark);
  assert.equal(missed.ok, false);
  assert.equal(missed.rows.length, 0);
});

test("a later total never drops below the snapshot", () => {
  const previous = {
    day: "2026-11-13",
    paid: 40,
    refused: 58,
    agentIds: ["a.testnet", "b.testnet"],
    protectedYocto: "5000",
    attemptsToday: 12,
  };
  const raised = raiseScoreboard(previous, {
    day: "2026-11-13",
    paid: 14,
    refused: 36,
    agentIds: ["a.testnet"],
    protectedYocto: "1000",
    attemptsToday: 4,
    asOf: "2026-11-13T06:00:00.000Z",
  });
  assert.equal(raised.paid, 40);
  assert.equal(raised.refused, 58);
  assert.deepEqual(raised.agentIds, ["a.testnet", "b.testnet"]);
  assert.equal(raised.protectedYocto, "5000");
  assert.equal(raised.attemptsToday, 12);
  const shown = presentScoreboard(raised, Date.parse("2026-11-13T06:00:00.000Z"));
  assert.equal(shown.attemptsRemaining, 288);
  assert.equal(shown.truncated, false);
  assert.equal(shown.agents, 2);
  assert.match(shown.protectedDisplay, /wNEAR$/);
});

test("a new UTC day does not keep yesterday's attempt count", () => {
  const raised = raiseScoreboard(
    { day: "2026-11-12", paid: 4, refused: 4, agentIds: [], protectedYocto: "0", attemptsToday: 40 },
    { day: "2026-11-13", paid: 5, refused: 4, agentIds: [], protectedYocto: "0", attemptsToday: 1 },
  );
  assert.equal(raised.attemptsToday, 1);
  assert.equal(raised.paid, 5);
});

test("an RPC status amount is the pay argument", () => {
  const args = Buffer.from(JSON.stringify({ amount: "1000" })).toString("base64");
  assert.equal(amountFromRpcStatus({
    result: { transaction: { actions: [{ FunctionCall: { method_name: "pay", args } }] } },
  }), "1000");
});

test("value protected skips an overflow probe and keeps a real refusal", () => {
  assert.equal(sumProtected({
    dust: "1000",
    overflow: (2n ** 128n - 1n).toString(),
  }), "1000");
});

test("a transaction detail amount is the pay argument", () => {
  assert.equal(amountFromTxn({
    data: { actions: [{ args: { args_json: { amount: "1000" } } }] },
  }), "1000");
  assert.equal(amountFromTxn({ data: { actions: [{ args: { args_json: { amount: "1.5" } } }] } }), null);
});

test("the committed snapshot is one complete board", () => {
  const board = JSON.parse(readFileSync(join(root, "public", "scoreboard.json"), "utf8"));
  assert.equal(board.truncated, false);
  assert.equal(board.complete, true);
  assert.equal(typeof board.paid, "number");
  assert.equal(typeof board.refused, "number");
  assert.equal(typeof board.attemptsRemaining, "number");
  assert.match(board.asOf, /^\d{4}-\d{2}-\d{2}T/);
  assert.equal(board.networks.testnet.agent, "tpd-e947e7-agent.testnet");
  assert.equal(board.networks.mainnet.agent, "tp-agent2.hlpok1lad.tg");
  assert.equal(existsSync(join(root, "..", "..", ".github", "workflows", "scoreboard.yml")), false);
});

test("a bare receipt image route returns the default card", async () => {
  const png = await liveGet(new Request("http://demo.local/api/live?route=og"));
  assert.equal(png.status, 200);
  assert.equal(png.headers.get("content-type"), "image/png");
  const extra = await liveGet(new Request("http://demo.local/api/live?route=og&utm=judge"));
  assert.equal(extra.status, 200);
  assert.equal(extra.headers.get("content-type"), "image/png");
  const share = await liveGet(new Request("http://demo.local/api/live?route=share"));
  assert.equal(share.status, 200);
  assert.match(await share.text(), /og:image/);
});

test("the limiter count is what a chain reservation reports after the sign", () => {
  assert.equal(signedAttemptCount({ source: "chain", attemptsToday: 50 }), 51);
  assert.equal(signedAttemptCount({ source: "shared", attemptsToday: 51 }), 51);
  assert.equal(persistedAttempts([
    { day: "2026-11-13", attemptsToday: 50 },
    { day: "2026-11-13", attemptsToday: 58 },
    { day: "2026-11-12", attemptsToday: 90 },
  ], Date.parse("2026-11-13T08:00:00.000Z")), 58);
});

test("a failed index walk keeps the persisted board and the limiter count", async () => {
  const board = await scoreboard({
    nowMs: Date.parse("2026-11-13T08:00:00.000Z"),
    snapshot: {
      paid: 79,
      refused: 100,
      attemptsToday: 50,
      day: "2026-11-13",
      agentIds: ["a.testnet"],
      protectedYocto: "10",
      networks: { testnet: { newestHash: "old", newestAtMs: 1 } },
      asOf: "2026-11-13T06:15:00.000Z",
    },
    readBoards: async () => [{
      paid: 81,
      refused: 106,
      attemptsToday: 58,
      day: "2026-11-13",
      agentIds: ["a.testnet"],
      protectedYocto: "20",
      networks: { testnet: { newestHash: "newer", newestAtMs: 5 } },
    }],
    writeBoard: async () => {},
    fetchPage: async () => ({ ok: false, rows: [], next: null }),
    fetchAmount: async () => null,
    attemptCount: 58,
  });
  assert.equal(board.paid, 81);
  assert.equal(board.refused, 106);
  assert.equal(board.attemptsToday, 58);
  assert.equal(board.attemptsRemaining, 242);
  assert.equal(board.truncated, false);
});

test("the home document takes its numbers from the live board", async () => {
  const sample = fillHome(
    '<p id="num-paid" class="stat">1</p><p id="num-refused" class="stat">1</p><p id="num-protected" class="stat">0</p><p id="num-agents" class="stat">0</p><p id="attempt-left" class="hint">1 testnet attempts left today</p><p id="num-source" class="hint">old</p><ol id="tape" class="tape"></ol>',
    {
      paid: 85,
      refused: 113,
      agents: 2,
      protectedDisplay: "0.009 wNEAR",
      attemptsRemaining: 221,
      asOf: "2026-11-13T07:00:00.000Z",
      recent: [{ hash: "5BCGjxC4CkPSy8iv876cP65xxs2LRWwcWKFqmcVJEyEw", network: "mainnet", paid: false, atMs: 10 }],
    },
  );
  assert.match(sample, /id="num-paid" class="stat">85</);
  assert.match(sample, /221 testnet attempts left today/);
  assert.match(sample, /as of 07:00 UTC/);
  assert.match(sample, /REFUSED/);
  assert.match(sample, /nearblocks\.io\/txns\/5BCGjxC4/);
  const tape = tapeFromRows([{
    network: "testnet",
    rows: [
      { txHash: "abcdefghij123456", methods: ["pay"], success: false, atMs: 2 },
      { txHash: "abcdefghij123456", methods: ["pay"], success: true, atMs: 9 },
      { txHash: "zzzzzzzzzz123456", methods: ["set_agent"], success: true, atMs: 3 },
    ],
  }]);
  assert.equal(tape.length, 1);
  assert.equal(tape[0].paid, false);
  const home = await liveGet(new Request("http://demo.local/api/live?route=home"));
  assert.equal(home.status, 200);
  assert.match(home.headers.get("cache-control") || "", /s-maxage=10/);
  assert.match(home.headers.get("cache-control") || "", /stale-while-revalidate/);
  assert.match(await home.text(), /id="tape"/);
  assert.equal(HOME_CACHE.includes("s-maxage=10"), true);
});

test("a signed attempt is stored above the snapshot floor", async () => {
  let stored = null;
  await publishAttempts(58, Date.parse("2026-11-13T08:00:00.000Z"), {
    floor: { paid: 79, refused: 100, attemptsToday: 50, day: "2026-11-13", protectedYocto: "10", agentIds: [] },
    readBoards: async () => [],
    writeBoard: async (board) => { stored = board; },
  });
  assert.equal(stored.attemptsToday, 58);
  assert.equal(stored.paid, 79);
});
