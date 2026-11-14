// The board the first paint uses. Blob is the live copy. The baked
// snapshot is only the fallback when Blob does not answer.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { readBlobBoard, writeBoard } from "./board-store.js";
import { fetchIndexerPage } from "./facts.js";
import { presentScoreboard, raiseScoreboard, readSnapshot, SIDES } from "./scoreboard.js";
import { fillHome, tapeFromRows } from "./tape.js";
import { fetchPayTransaction } from "./txread.js";

const PAGE = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "shell.html");
export const HOME_CACHE = "public, max-age=0, s-maxage=10, stale-while-revalidate=60";

export async function liveBoard() {
  const snap = readSnapshot();
  const blob = await readBlobBoard();
  if (!blob) return presentScoreboard(snap || {});
  const raised = raiseScoreboard(snap || blob, blob);
  return presentScoreboard({
    ...raised,
    recent: Array.isArray(blob.recent) && blob.recent.length ? blob.recent : (snap && snap.recent) || [],
    networks: raised.networks || blob.networks || {},
  });
}

export async function peekTape(timeoutMs = 900) {
  const job = Promise.all(SIDES.map(async (side) => {
    const page = await fetchIndexerPage(side.indexerAccountsBase, side.agent, { limit: 8 });
    return { network: side.network, rows: page.rows || [] };
  })).then((groups) => tapeFromRows(groups));
  const timeout = new Promise((resolve) => setTimeout(() => resolve([]), timeoutMs));
  try {
    return await Promise.race([job, timeout]);
  } catch {
    return [];
  }
}

/**
 * Add the payee and the refusal code to ticker rows that lack them, read
 * from each transaction. Reads are cached per process and the whole step
 * is time-boxed, so the page never waits on the chain for its ticker.
 */
export async function enrichTape(rows, budgetMs = 1200) {
  const list = Array.isArray(rows) ? rows : [];
  const sides = new Map(SIDES.map((side) => [side.network, side]));
  const jobs = list.map(async (row) => {
    if (!row || !row.hash || (row.payee && (row.paid || row.code || row.held))) return row;
    const side = sides.get(row.network === "mainnet" ? "mainnet" : "testnet");
    if (!side) return row;
    const pay = await fetchPayTransaction(side.rpcUrl, row.hash, [side.agent]).catch(() => null);
    if (!pay) return row;
    return {
      ...row,
      payee: row.payee || pay.payee || null,
      code: row.code || pay.errorCode || null,
      held: Boolean(row.held || pay.held),
    };
  });
  const timeout = new Promise((resolve) => setTimeout(() => resolve(null), budgetMs));
  const done = await Promise.race([Promise.all(jobs), timeout]);
  return Array.isArray(done) ? done : list;
}

export async function boardPayload() {
  const board = await liveBoard();
  const stale = !board.recent || !board.recent.length;
  const base = stale ? await peekTape() : board.recent;
  if (!base.length) return board;
  const recent = await enrichTape(base);
  const next = { ...board, recent };
  const changed = stale || recent.some((row, index) => row !== base[index]);
  if (changed) await writeBoard(next).catch(() => {});
  return next;
}

export function renderHomeHtml(board) {
  return fillHome(readFileSync(PAGE, "utf8"), board);
}

export async function homeResponse() {
  const board = await liveBoard();
  let recent = Array.isArray(board.recent) ? board.recent : [];
  if (recent.length < 4) {
    const peeked = await peekTape(400);
    if (peeked.length > recent.length) recent = peeked;
  }
  recent = await enrichTape(recent, 700);
  return new Response(renderHomeHtml({ ...board, recent }), {
    status: 200,
    headers: {
      "content-type": "text/html; charset=utf-8",
      "cache-control": HOME_CACHE,
    },
  });
}
