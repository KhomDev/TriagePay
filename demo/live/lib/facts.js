// Chain facts for the risk check. Code comes from testnet RPC
// `view_account`. Age comes from the account's creation transaction via
// RPC when the signer is known, and otherwise from the testnet index.

import { viewAccount, txTimeMs } from "./near.js";
import { parseIndexerAccount, parseIndexerTxns } from "./recent.js";

async function getJson(url) {
  const response = await fetch(url, {
    headers: { accept: "application/json" },
    signal: AbortSignal.timeout(8000),
  });
  if (!response.ok) return null;
  return response.json();
}

export async function fetchIndexerAccount(base, accountId) {
  if (!base || !accountId) return null;
  try {
    const body = await getJson(base + encodeURIComponent(accountId));
    return body ? parseIndexerAccount(body) : null;
  } catch {
    return null;
  }
}

export async function fetchIndexerTxns(base, accountId) {
  const page = await fetchIndexerPage(base, accountId, { limit: 8 });
  return page.rows;
}

export async function fetchIndexerPage(base, accountId, { limit = 25, next = null } = {}) {
  if (!base || !accountId) return { rows: [], next: null, ok: false };
  try {
    const url = new URL(`${base}${encodeURIComponent(accountId)}/txns`);
    url.searchParams.set("limit", String(limit));
    if (next) url.searchParams.set("next", next);
    const body = await getJson(url);
    if (!body) return { rows: [], next: null, ok: false };
    const cursor = body.meta && typeof body.meta.next_page === "string" ? body.meta.next_page : null;
    return { rows: parseIndexerTxns(body), next: cursor, ok: true };
  } catch {
    return { rows: [], next: null, ok: false };
  }
}

// Walk the index until a transaction is older than the UTC day, or until
// `maxPages`. `truncated` means the day may contain more transactions than
// the pages that were read. `unavailable` means the index could not be read.
export async function fetchDayStamps(base, accountId, nowMs, { maxPages = 16, limit = 25, stopAt = Infinity } = {}) {
  const dayStart = Date.UTC(
    new Date(nowMs).getUTCFullYear(),
    new Date(nowMs).getUTCMonth(),
    new Date(nowMs).getUTCDate(),
  );
  const stamps = [];
  let cursor = null;
  for (let page = 0; page < maxPages; page++) {
    const batch = await fetchIndexerPage(base, accountId, { limit, next: cursor });
    if (!batch.ok) return { stamps, truncated: false, unavailable: true };
    let crossed = false;
    for (const row of batch.rows) {
      if (row.atMs == null) continue;
      if (row.atMs < dayStart) {
        crossed = true;
        break;
      }
      stamps.push(row.atMs);
      if (stamps.length >= stopAt) return { stamps, truncated: false, unavailable: false };
    }
    if (crossed || !batch.next) return { stamps, truncated: false, unavailable: false };
    cursor = batch.next;
  }
  return { stamps, truncated: true, unavailable: false };
}

export async function payeeFacts({ rpcUrl, accountId, creation, nowMs, indexerBase }) {
  const account = await viewAccount(rpcUrl, accountId);
  let createdAtMs = null;
  let ageSource = "unknown";
  if (account && creation && creation.hash && creation.signer) {
    try {
      createdAtMs = await txTimeMs(rpcUrl, creation.hash, creation.signer);
      if (createdAtMs != null) ageSource = "rpc";
    } catch {
      createdAtMs = null;
    }
  }
  if (account && createdAtMs == null) {
    const indexed = await fetchIndexerAccount(indexerBase, accountId);
    if (indexed && indexed.createdAtMs != null) {
      createdAtMs = indexed.createdAtMs;
      ageSource = "indexer";
    }
  }
  return {
    accountId,
    exists: Boolean(account),
    codeHash: account ? account.code_hash : null,
    createdAtMs,
    ageSource,
    nowMs,
  };
}
