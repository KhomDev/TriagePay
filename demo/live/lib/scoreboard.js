// One scoreboard for every client. A snapshot file is the floor. A live
// read may add transactions newer than that snapshot, and the totals never
// go down. The counted set is fixed: pay calls by the two demo agents,
// deduped by transaction hash. A successful pay is paid. Any other pay is
// refused. Value protected sums refused amounts up to 1000 wNEAR each.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { readAttemptCounter, readBoards, writeBoard, boardRose } from "./board-store.js";
import { tapeFromRows } from "./tape.js";
import { utcDayStartMs } from "./cooldown.js";
import { fetchIndexerPage } from "./facts.js";
import { formatAmountShort } from "./format.js";

export const PROTECTED_MAX = 10n ** 27n;

export function sumProtected(amounts) {
  let total = 0n;
  for (const value of Object.values(amounts || {})) {
    if (typeof value !== "string" || !/^\d+$/.test(value)) continue;
    const amount = BigInt(value);
    if (amount <= PROTECTED_MAX) total += amount;
  }
  return total.toString();
}

function usableProtected(value) {
  try {
    const amount = BigInt(value || "0");
    if (amount < 0n || amount > PROTECTED_MAX * 10000n) return 0n;
    return amount;
  } catch {
    return 0n;
  }
}

export const MAX_PAGES = 80;
export const PAGE_LIMIT = 25;
export const LIVE_PAGES = 4;
export const MAX_ATTEMPTS_PER_DAY = 300;
const SNAPSHOT_PATH = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "scoreboard.json");

export const SIDES = [
  {
    network: "testnet",
    agent: "tpd-e947e7-agent.testnet",
    indexerAccountsBase: "https://api-testnet.nearblocks.io/v3/accounts/",
    txBase: "https://api-testnet.nearblocks.io/v3/txns/",
    rpcUrl: "https://test.rpc.fastnear.com",
  },
  {
    network: "mainnet",
    agent: "tp-agent2.hlpok1lad.tg",
    indexerAccountsBase: "https://api.nearblocks.io/v3/accounts/",
    txBase: "https://api.nearblocks.io/v3/txns/",
    rpcUrl: "https://rpc.mainnet.fastnear.com",
  },
];

export function readSnapshot() {
  try {
    const parsed = JSON.parse(readFileSync(SNAPSHOT_PATH, "utf8"));
    if (!parsed || typeof parsed !== "object") return null;
    return parsed;
  } catch {
    return null;
  }
}

export function amountFromRpcStatus(body) {
  const tx = body && body.result && body.result.transaction;
  const actions = tx && Array.isArray(tx.actions) ? tx.actions : [];
  const call = actions.map((action) => action && action.FunctionCall).find(Boolean);
  if (!call || typeof call.args !== "string") return null;
  try {
    const args = JSON.parse(Buffer.from(call.args, "base64").toString("utf8"));
    const amount = args && args.amount;
    if (typeof amount === "string" && /^\d+$/.test(amount)) return amount;
  } catch {
    return null;
  }
  return null;
}

export function amountFromTxn(body) {
  const data = body && body.data ? body.data : body;
  const actions = data && Array.isArray(data.actions) ? data.actions : [];
  for (const action of actions) {
    const args = action && action.args && action.args.args_json;
    const amount = args && args.amount;
    if (typeof amount === "string" && /^\d+$/.test(amount)) return amount;
    if (typeof amount === "number" && Number.isSafeInteger(amount) && amount >= 0) return String(amount);
  }
  return null;
}

// Pages are newest first. `complete` is true only when the cursor ends
// inside the page bound. A partial walk is not a total.
export function takeAll(pages, maxPages = MAX_PAGES) {
  const rows = [];
  const seen = new Set();
  let used = 0;
  for (const page of pages || []) {
    used += 1;
    if (!page || !page.ok) return { complete: false, rows };
    for (const row of page.rows || []) {
      if (!row || !row.txHash || seen.has(row.txHash)) continue;
      seen.add(row.txHash);
      rows.push(row);
    }
    if (!page.next) return { complete: true, rows };
    if (used >= maxPages) return { complete: false, rows };
  }
  return { complete: false, rows };
}

// Keep rows strictly newer than the snapshot watermark. If the watermark
// is not on the pages, the walk is not usable: adding those rows would
// count the snapshot again.
export function takeNewer(pages, watermark, maxPages = LIVE_PAGES) {
  if (!watermark || !watermark.newestHash) return { ok: false, rows: [] };
  const rows = [];
  let used = 0;
  for (const page of pages || []) {
    used += 1;
    if (!page || !page.ok) return { ok: false, rows: [] };
    for (const row of page.rows || []) {
      if (!row || !row.txHash) continue;
      const older = row.txHash === watermark.newestHash
        || (watermark.newestAtMs && row.atMs != null && row.atMs <= watermark.newestAtMs);
      if (older) return { ok: true, rows };
      rows.push(row);
    }
    if (!page.next) return { ok: false, rows: [] };
    if (used >= maxPages) return { ok: false, rows: [] };
  }
  return { ok: false, rows: [] };
}

export function tallyPaySet(rows, dayStartMs = null) {
  const seen = new Set();
  const agents = new Set();
  const refusedHashes = [];
  let paid = 0;
  let refused = 0;
  let attemptsToday = 0;
  let newest = null;
  for (const row of rows || []) {
    if (!row || !row.txHash || seen.has(row.txHash)) continue;
    seen.add(row.txHash);
    if (row.atMs != null && (!newest || row.atMs > newest.atMs)) {
      newest = { hash: row.txHash, atMs: row.atMs };
    }
    if (dayStartMs != null && row.atMs != null && row.atMs >= dayStartMs) attemptsToday += 1;
    if (!(row.methods || []).includes("pay")) continue;
    if (row.signer) agents.add(row.signer);
    if (row.success) paid += 1;
    else {
      refused += 1;
      refusedHashes.push(row.txHash);
    }
  }
  return {
    paid,
    refused,
    agentIds: [...agents].sort(),
    refusedHashes,
    attemptsToday,
    newest,
  };
}

export function raiseScoreboard(previous, fresh) {
  const agents = new Set(previous && Array.isArray(previous.agentIds) ? previous.agentIds : []);
  for (const id of fresh.agentIds || []) agents.add(id);
  const prevYocto = usableProtected(previous && previous.protectedYocto);
  const nextYocto = usableProtected(fresh.protectedYocto);
  const sameDay = Boolean(previous && previous.day && previous.day === fresh.day);
  const agentIds = [...agents].sort();
  return {
    ...fresh,
    paid: Math.max(previous && previous.paid || 0, fresh.paid || 0),
    refused: Math.max(previous && previous.refused || 0, fresh.refused || 0),
    agentIds,
    agents: agentIds.length,
    protectedYocto: (prevYocto > nextYocto ? prevYocto : nextYocto).toString(),
    attemptsToday: sameDay
      ? Math.max(previous.attemptsToday || 0, fresh.attemptsToday || 0)
      : fresh.attemptsToday,
  };
}

export function presentScoreboard(board, nowMs = Date.now()) {
  const source = board || {};
  const attemptsToday = Number.isFinite(source.attemptsToday) ? source.attemptsToday : null;
  const maxAttempts = source.maxAttemptsPerDay || MAX_ATTEMPTS_PER_DAY;
  const yocto = source.protectedYocto || "0";
  const agentIds = Array.isArray(source.agentIds) ? source.agentIds : [];
  const agents = agentIds.length ? agentIds.length : (Number.isFinite(source.agents) ? source.agents : 0);
  return {
    paid: source.paid || 0,
    refused: source.refused || 0,
    agents,
    agentIds,
    protectedYocto: String(yocto),
    protectedDisplay: formatAmountShort(yocto, 24, "wNEAR"),
    attemptsToday,
    attemptsRemaining: attemptsToday == null ? null : Math.max(0, maxAttempts - attemptsToday),
    maxAttemptsPerDay: maxAttempts,
    truncated: false,
    complete: source.complete !== false,
    asOf: source.asOf || new Date(nowMs).toISOString(),
    day: source.day || new Date(nowMs).toISOString().slice(0, 10),
    source: "Live from NearBlocks",
    sourceUrl: source.sourceUrl || "https://nearblocks.io",
    recent: Array.isArray(source.recent) ? source.recent : [],
    networks: source.networks || {},
    definition: source.definition || "Pay calls by the two demo agents, deduped by transaction hash.",
  };
}

async function defaultFetchAmount(side, hash) {
  const response = await fetch(side.txBase + hash, {
    headers: { accept: "application/json" },
    signal: AbortSignal.timeout(8000),
  });
  if (!response.ok) return null;
  return amountFromTxn(await response.json());
}

function newestNetworks(boards) {
  const networks = {};
  for (const board of boards) {
    const nets = board && board.networks;
    if (!nets) continue;
    for (const key of Object.keys(nets)) {
      const next = nets[key];
      const prev = networks[key];
      if (!next) continue;
      if (!prev || (next.newestAtMs || 0) > (prev.newestAtMs || 0)) networks[key] = next;
    }
  }
  return networks;
}

async function pagesUntil(fetchPage, side, watermark) {
  const pages = [];
  let cursor = null;
  let head = [];
  for (let i = 0; i < LIVE_PAGES; i += 1) {
    const page = await fetchPage(side, cursor);
    if (i === 0) head = (page && page.rows) || [];
    pages.push(page);
    const taken = takeNewer(pages, watermark, LIVE_PAGES);
    if (taken.ok || !page || !page.ok || !page.next) return { ...taken, head };
    cursor = page.next;
  }
  return { ...takeNewer(pages, watermark, LIVE_PAGES), head };
}

export async function publishAttempts(count, nowMs = Date.now(), options = {}) {
  const day = new Date(nowMs).toISOString().slice(0, 10);
  const read = options.readBoards || readBoards;
  const write = options.writeBoard || writeBoard;
  const saved = await read();
  let merged = options.floor || readSnapshot() || {
    paid: 0,
    refused: 0,
    protectedYocto: "0",
    agentIds: [],
    attemptsToday: 0,
    day,
  };
  for (const board of saved) {
    if (board) merged = raiseScoreboard(merged, board);
  }
  merged = raiseScoreboard(merged, {
    ...merged,
    day,
    attemptsToday: count,
    asOf: new Date(nowMs).toISOString(),
    networks: newestNetworks([merged, ...saved]),
  });
  const keptRecent = [merged, ...saved].reduce((best, board) => {
    const rows = board && Array.isArray(board.recent) ? board.recent : [];
    return rows.length > best.length ? rows : best;
  }, []);
  merged = { ...merged, recent: keptRecent };
  await write(merged);
  return merged;
}

export async function scoreboard(options = {}) {
  const nowMs = options.nowMs || Date.now();
  const snapshot = options.snapshot === undefined ? readSnapshot() : options.snapshot;
  const read = options.readBoards || readBoards;
  const write = options.writeBoard || writeBoard;
  const saved = await read().catch(() => []);
  let floor = snapshot || {
    paid: 0,
    refused: 0,
    agentIds: [],
    protectedYocto: "0",
    attemptsToday: null,
    day: null,
    networks: {},
    asOf: new Date(nowMs).toISOString(),
  };
  for (const board of saved || []) {
    if (board) floor = raiseScoreboard(floor, board);
  }
  floor = { ...floor, networks: newestNetworks([floor, snapshot, ...(saved || [])]) };
  const fetchPage = options.fetchPage || ((side, cursor) => fetchIndexerPage(side.indexerAccountsBase, side.agent, {
    limit: PAGE_LIMIT,
    next: cursor,
  }));
  const fetchAmount = options.fetchAmount === undefined ? defaultFetchAmount : options.fetchAmount;
  const day = new Date(nowMs).toISOString().slice(0, 10);
  const sameDay = floor.day === day;
  let paid = 0;
  let refused = 0;
  let attempts = 0;
  let testnetWalked = false;
  let extraYocto = 0n;
  let added = false;
  const agents = new Set(Array.isArray(floor.agentIds) ? floor.agentIds : []);
  const networks = { ...(floor.networks || {}) };
  const heads = [];
  for (const side of SIDES) {
    const mark = networks[side.network];
    let walked = { ok: false, rows: [], head: [] };
    try {
      walked = await pagesUntil(fetchPage, side, mark);
    } catch {
      walked = { ok: false, rows: [], head: [] };
    }
    if (walked.head && walked.head.length) heads.push({ network: side.network, rows: walked.head });
    if (side.network === "testnet" && walked.ok) testnetWalked = true;
    if (!walked.ok || !walked.rows.length) continue;
    const tally = tallyPaySet(walked.rows, side.network === "testnet" ? utcDayStartMs(nowMs) : null);
    paid += tally.paid;
    refused += tally.refused;
    if (side.network === "testnet") attempts += tally.attemptsToday;
    for (const id of tally.agentIds) agents.add(id);
    if (tally.newest) {
      networks[side.network] = {
        ...(networks[side.network] || {}),
        newestHash: tally.newest.hash,
        newestAtMs: tally.newest.atMs,
      };
    }
    added = true;
    if (fetchAmount && tally.refusedHashes.length > 0 && tally.refusedHashes.length <= 12) {
      const amounts = await Promise.all(tally.refusedHashes.map((hash) => fetchAmount(side, hash).catch(() => null)));
      if (amounts.every(Boolean)) {
        for (const value of amounts) {
          const amount = BigInt(value);
          if (amount <= PROTECTED_MAX) extraYocto += amount;
        }
      }
    }
  }
  let attemptsToday = sameDay
    ? (floor.attemptsToday || 0) + attempts
    : (testnetWalked ? attempts : floor.attemptsToday);
  const limiter = options.attemptCount !== undefined
    ? options.attemptCount
    : await readAttemptCounter(nowMs).catch(() => null);
  if (limiter != null && (sameDay || testnetWalked)) {
    attemptsToday = Math.max(attemptsToday || 0, limiter);
  }
  const agentIds = [...agents].sort();
  const merged = {
    ...floor,
    paid: (floor.paid || 0) + paid,
    refused: (floor.refused || 0) + refused,
    agentIds,
    agents: agentIds.length,
    protectedYocto: (BigInt(floor.protectedYocto || "0") + extraYocto).toString(),
    attemptsToday,
    asOf: added ? new Date(nowMs).toISOString() : (floor.asOf || new Date(nowMs).toISOString()),
    day: attemptsToday == null ? floor.day : day,
    networks,
    recent: tapeFromRows(heads).length ? tapeFromRows(heads) : (floor.recent || []),
    complete: true,
    truncated: false,
  };
  const recentChanged = JSON.stringify(merged.recent || []) !== JSON.stringify(floor.recent || []);
  if (boardRose(floor, merged) || recentChanged) await write(merged).catch(() => {});
  return presentScoreboard(merged, nowMs);
}
