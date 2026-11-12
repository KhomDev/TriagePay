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
  if (!base || !accountId) return [];
  try {
    const body = await getJson(`${base}${encodeURIComponent(accountId)}/txns?limit=8`);
    return body ? parseIndexerTxns(body) : [];
  } catch {
    return [];
  }
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
