// NearBlocks v3 shapes. The RPC does not list an account's transactions,
// so the dashboard reads this index and still takes balances from the
// wallet's own view methods.

import { nsToMs } from "./format.js";

export function parseIndexerAccount(body) {
  const data = body && body.data;
  if (!data || !data.created) return null;
  const createdAtMs = nsToMs(data.created.block_timestamp);
  if (createdAtMs == null) return null;
  return {
    createdAtMs,
    creationHash: data.created.transaction_hash || null,
  };
}

export function parseIndexerTxns(body) {
  const rows = body && Array.isArray(body.data) ? body.data : [];
  return rows
    .map((row) => {
      const methods = Array.isArray(row.actions)
        ? row.actions.map((action) => action && action.method).filter(Boolean)
        : [];
      const status = row.outcomes && row.outcomes.status;
      return {
        txHash: row.transaction_hash || null,
        signer: row.signer_account_id || null,
        receiver: row.receiver_account_id || null,
        atMs: nsToMs(row.block_timestamp ?? row.block?.block_timestamp),
        success: status === true,
        methods,
      };
    })
    .filter((row) => row.txHash);
}

export function mergeRecent(indexed, journal, explorerBase) {
  const byHash = new Map();
  for (const row of indexed || []) {
    byHash.set(row.txHash, {
      txHash: row.txHash,
      atMs: row.atMs,
      outcome: row.success ? "success" : "refused",
      errorCode: null,
      kind: kindFromMethods(row.methods),
      title: row.methods.length ? row.methods.join(", ") : "transaction",
      payee: null,
      amount: null,
      riskVerdict: null,
      explorerUrl: explorerBase + row.txHash,
    });
  }
  for (const row of journal || []) {
    if (!row.txHash) continue;
    const prev = byHash.get(row.txHash) || {};
    byHash.set(row.txHash, {
      ...prev,
      txHash: row.txHash,
      atMs: row.atMs || prev.atMs || null,
      outcome: (row.outcome === "failure" ? "refused" : row.outcome) || (prev.outcome === "failure" ? "refused" : prev.outcome) || null,
      errorCode: row.errorCode || prev.errorCode || null,
      kind: row.kind || prev.kind || "chain",
      title: row.title || prev.title || "transaction",
      payee: row.payee || prev.payee || null,
      amount: row.amount || prev.amount || null,
      riskVerdict: row.riskVerdict || prev.riskVerdict || null,
      explorerUrl: explorerBase + row.txHash,
    });
  }
  return [...byHash.values()].sort((a, b) => (b.atMs || 0) - (a.atMs || 0)).slice(0, 12);
}

function kindFromMethods(methods) {
  if (methods.includes("revoke_agent")) return "revoke";
  if (methods.includes("set_agent")) return "restore";
  if (methods.includes("pay")) return "payment";
  return "chain";
}
