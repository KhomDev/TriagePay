import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { GET as liveGet } from "../api/live.js";
import {
  amountFromRpcStatus,
  amountFromTxn,
  presentScoreboard,
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
});

test("a bare receipt image route returns the default card", async () => {
  const png = await liveGet(new Request("http://demo.local/api/live?route=og"));
  assert.equal(png.status, 200);
  assert.equal(png.headers.get("content-type"), "image/png");
});
