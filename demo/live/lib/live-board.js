// The board the first paint uses. Blob is the live copy. The baked
// snapshot is only the fallback when Blob does not answer.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { readBlobBoard, writeBoard } from "./board-store.js";
import { fetchIndexerPage } from "./facts.js";
import { presentScoreboard, raiseScoreboard, readSnapshot, SIDES } from "./scoreboard.js";
import { fillHome, tapeFromRows } from "./tape.js";

const PAGE = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "index.html");
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

export async function boardPayload() {
  const board = await liveBoard();
  const stale = !board.recent || !board.recent.length;
  if (!stale) return board;
  const recent = await peekTape();
  if (!recent.length) return board;
  const next = { ...board, recent };
  await writeBoard(next).catch(() => {});
  return next;
}

export function renderHomeHtml(board) {
  return fillHome(readFileSync(PAGE, "utf8"), board);
}

export async function homeResponse() {
  const board = await liveBoard();
  return new Response(renderHomeHtml(board), {
    status: 200,
    headers: {
      "content-type": "text/html; charset=utf-8",
      "cache-control": HOME_CACHE,
    },
  });
}
