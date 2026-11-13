// A public profile of one agent on one wallet. Limits come from the
// wallet's view methods. Transaction rows come from the indexer. Error
// codes come from this server's journal when it signed the transaction.

import { formatAmount } from "./format.js";

export function buildPassport({
  accountId,
  wallet,
  agent,
  review,
  indexed,
  journal,
  explorerTx,
  explorerAccount,
  decimals,
  symbol,
}) {
  const view = agent || null;
  const byHash = new Map();
  for (const row of journal || []) {
    if (row && row.txHash) byHash.set(row.txHash, row);
  }
  const events = (indexed || []).map((row) => {
    const extra = byHash.get(row.txHash) || {};
    const methods = row.methods || [];
    let label = methods[0] || "transaction";
    if (methods.includes("revoke_agent")) label = "revoke";
    else if (methods.includes("approve_payment")) label = "approve";
    else if (methods.includes("reject_payment")) label = "reject";
    else if (methods.includes("set_agent")) label = "set_agent";
    else if (methods.includes("pay")) label = row.success ? "paid" : "refused";
    return {
      txHash: row.txHash,
      atMs: row.atMs || null,
      label,
      success: Boolean(row.success),
      errorCode: extra.errorCode || null,
      explorerUrl: explorerTx ? explorerTx + row.txHash : null,
    };
  });
  const refusals = events.filter((row) => row.label === "refused").length;
  const paid = events.filter((row) => row.label === "paid").length;
  const approved = events.filter((row) => row.label === "approve" && row.success).length;
  const revokes = events.filter((row) => row.label === "revoke").length;
  return {
    accountId,
    wallet,
    found: Boolean(view),
    source: {
      limits: view ? "wallet contract" : "wallet contract (no agent)",
      transactions: "indexer",
      errorCodes: "this server's journal, when it signed the transaction",
    },
    active: Boolean(view && view.active),
    perCall: view ? formatAmount(view.per_call_max, decimals, symbol) : null,
    daily: view ? formatAmount(view.daily_max, decimals, symbol) : null,
    spent: view ? formatAmount(view.spent_in_window || "0", decimals, symbol) : null,
    allowlist: view && Array.isArray(view.payees) ? view.payees.map(String) : [],
    approvalThreshold: review && review.supported
      ? formatAmount(review.approvalThreshold || "0", decimals, symbol)
      : null,
    counts: { refusals, paid, approved, revokes },
    events: events.slice(0, 12),
    explorerUrl: explorerAccount && accountId ? explorerAccount + accountId : null,
    walletUrl: explorerAccount && wallet ? explorerAccount + wallet : null,
  };
}
