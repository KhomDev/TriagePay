// One board for every client. KV/Upstash is the rate-limiter store. Vercel
// Blob is the shared copy when that token is set. The region cache is the
// fallback, and it is the same value the signer writes.

import { cacheRead, cacheSet } from "./region-cache.js";
import { dayKey } from "./limits.js";
import { storeFromEnv } from "./store.js";

const KEY = "triagepay:board";
const BLOB_PATH = "triagepay-board.json";
const memory = { board: null };

export function resetBoardStore() {
  memory.board = null;
}

export function persistedAttempts(boards, nowMs) {
  const day = new Date(nowMs).toISOString().slice(0, 10);
  let max = null;
  for (const board of boards || []) {
    if (!board || board.day !== day || !Number.isFinite(board.attemptsToday)) continue;
    max = max == null ? board.attemptsToday : Math.max(max, board.attemptsToday);
  }
  return max;
}

export function signedAttemptCount(decision) {
  const seen = Number(decision && decision.attemptsToday) || 0;
  if (decision && String(decision.source || "").startsWith("shared")) return seen;
  return seen + 1;
}

function parseBoard(raw) {
  if (!raw) return null;
  if (typeof raw === "object") return raw;
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    return null;
  }
}

export function compactBoard(board) {
  if (!board || typeof board !== "object") return null;
  return {
    paid: board.paid || 0,
    refused: board.refused || 0,
    agents: board.agents || 0,
    agentIds: Array.isArray(board.agentIds) ? board.agentIds : [],
    protectedYocto: String(board.protectedYocto || "0"),
    protectedDisplay: board.protectedDisplay || null,
    attemptsToday: Number.isFinite(board.attemptsToday) ? board.attemptsToday : null,
    maxAttemptsPerDay: board.maxAttemptsPerDay || 300,
    day: board.day || null,
    asOf: board.asOf || null,
    networks: board.networks || {},
    complete: true,
    truncated: false,
    sourceUrl: board.sourceUrl || "https://nearblocks.io",
    recent: sanitizeRecent(board.recent),
  };
}

function sanitizeRecent(rows) {
  if (!Array.isArray(rows)) return [];
  const seen = new Set();
  const items = [];
  for (const row of rows) {
    const hash = row && typeof row.hash === "string" ? row.hash : "";
    if (!/^[A-Za-z0-9]{8,88}$/.test(hash) || seen.has(hash)) continue;
    seen.add(hash);
    items.push({
      hash,
      network: row.network === "mainnet" ? "mainnet" : "testnet",
      paid: Boolean(row.paid),
      atMs: Number.isFinite(row.atMs) ? row.atMs : 0,
      payee: /^[a-z0-9._-]{2,64}$/.test(String(row.payee || "")) ? String(row.payee) : null,
      code: /^ERR_[A-Z_]{3,40}$/.test(String(row.code || "")) ? String(row.code) : null,
      held: Boolean(row.held),
    });
  }
  items.sort((a, b) => b.atMs - a.atMs);
  return items.slice(0, 6);
}

export async function readBlobBoard() {
  const token = String(process.env.BLOB_READ_WRITE_TOKEN || "").trim();
  if (!token) return null;
  try {
    const { get } = await import("@vercel/blob");
    const result = await get(BLOB_PATH, {
      access: "public",
      token,
      useCache: false,
      abortSignal: AbortSignal.timeout(1500),
    });
    if (!result || !result.stream) return null;
    return parseBoard(await new Response(result.stream).text());
  } catch {
    return null;
  }
}

async function writeBlob(board) {
  const token = String(process.env.BLOB_READ_WRITE_TOKEN || "").trim();
  if (!token) return;
  try {
    const { put } = await import("@vercel/blob");
    await put(BLOB_PATH, JSON.stringify(board), {
      access: "public",
      token,
      addRandomSuffix: false,
      allowOverwrite: true,
      contentType: "application/json",
    });
  } catch {
    // KV or the region cache still holds the board.
  }
}

export async function readBoards() {
  const boards = [];
  if (memory.board) boards.push(memory.board);
  const cached = await cacheRead(KEY);
  if (cached) boards.push(cached);
  const store = storeFromEnv();
  if (store) {
    try {
      const parsed = parseBoard(await store.get(KEY));
      if (parsed) boards.push(parsed);
    } catch {
      // The rate limiter store did not answer. The other copies remain.
    }
  }
  const blob = await readBlobBoard();
  if (blob) boards.push(blob);
  return boards;
}

export async function readAttemptCounter(nowMs, store = storeFromEnv()) {
  if (!store) return null;
  try {
    const count = Number(await store.get(dayKey(nowMs)));
    return Number.isFinite(count) ? count : null;
  } catch {
    return null;
  }
}

export function boardRose(previous, next) {
  if (!previous) return true;
  if ((next.paid || 0) > (previous.paid || 0)) return true;
  if ((next.refused || 0) > (previous.refused || 0)) return true;
  if ((next.attemptsToday || 0) > (previous.attemptsToday || 0)) return true;
  try {
    if (BigInt(next.protectedYocto || "0") > BigInt(previous.protectedYocto || "0")) return true;
  } catch {
    return false;
  }
  return false;
}

export async function writeBoard(board) {
  const compact = compactBoard(board);
  if (!compact) return null;
  memory.board = compact;
  await cacheSet(KEY, compact, 24 * 60 * 60);
  const store = storeFromEnv();
  if (store) {
    try {
      await store.set(KEY, JSON.stringify(compact));
    } catch {
      // The region copy is still there.
    }
  }
  await writeBlob(compact);
  return compact;
}
