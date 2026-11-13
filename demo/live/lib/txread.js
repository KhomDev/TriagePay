// Read a pay transaction back from RPC. The args are base64 JSON on the
// action. The error code is in the receipt failure. A process cache keeps
// a batch of lookups from hitting the RPC twice.

import { describeAttack } from "./attacks.js";
import { fetchIndexerPage } from "./facts.js";
import { formatAtomic } from "./format.js";
import { historyRpc } from "./near.js";
import { RULE_TEXT } from "./receipt.js";

const resolved = new Map();
const inflight = new Map();

export function cachedPay(hash) {
  return resolved.get(hash) || null;
}

export function resetTxCacheForTests() {
  resolved.clear();
  inflight.clear();
}

export function decodeCallArgs(encoded) {
  if (typeof encoded !== "string" || !encoded) return null;
  try {
    const value = JSON.parse(Buffer.from(encoded, "base64").toString("utf8"));
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    return value;
  } catch {
    return null;
  }
}

export function readPayTransaction(status) {
  const tx = status && status.transaction;
  if (!tx) return null;
  const actions = Array.isArray(tx.actions) ? tx.actions : [];
  const call = actions.map((action) => action && action.FunctionCall).find(Boolean);
  if (!call || call.method_name !== "pay") return null;
  const args = decodeCallArgs(call.args) || {};
  const all = JSON.stringify(status);
  const match = all.match(/ERR_[A-Z0-9_]+/);
  const failure = Boolean(status.status && status.status.Failure);
  const held = !failure && /payment_queued|PaymentQueued/i.test(all);
  return {
    signer: tx.signer_id || null,
    receiver: tx.receiver_id || null,
    method: "pay",
    payee: args.receiver_id ? String(args.receiver_id) : null,
    amount: args.amount != null ? String(args.amount) : null,
    memo: args.memo ? String(args.memo) : null,
    token: args.token ? String(args.token) : null,
    outcome: failure ? "failure" : "success",
    errorCode: match ? match[0] : null,
    held,
  };
}

export function narrativeForPay(config, pay) {
  const payee = String(pay.payee || "").toLowerCase();
  const memo = String(pay.memo || "");
  const memoId = memo.startsWith("demo:") ? memo.slice(5) : "";
  const byMemo = memoId ? describeAttack(memoId, config) : null;
  if (byMemo && (byMemo.injection || byMemo.narrative)) {
    return {
      injection: byMemo.injection || byMemo.narrative,
      injectionFrom: "chain-memo",
    };
  }
  const injected = String(config.accounts.injected || "").toLowerCase();
  const flagged = String(config.accounts.flagged || "").toLowerCase();
  if (payee && payee === injected) {
    return { injection: describeAttack("injected-payee", config).injection, injectionFrom: "chain-payee" };
  }
  if (payee && payee === flagged) {
    return { injection: describeAttack("flagged-payee", config).injection, injectionFrom: "chain-payee" };
  }
  if (payee) return { injection: `The transaction asks the wallet to pay ${payee}.`, injectionFrom: "chain-args" };
  return { injection: "The transaction calls the wallet.", injectionFrom: "chain-args" };
}

export function replayFromChain(config, hash, pay) {
  let amountDisplay = null;
  if (pay.amount) {
    try {
      amountDisplay = formatAtomic(pay.amount, config.policy.decimals);
    } catch {
      amountDisplay = null;
    }
  }
  const story = narrativeForPay(config, pay);
  const code = pay.errorCode;
  const kind = pay.held ? "held" : pay.outcome === "failure" || code ? "refused" : "settled";
  const ruleText = code && RULE_TEXT[code] ? RULE_TEXT[code] : null;
  const payeeNote = story.injectionFrom === "chain-payee"
    ? "The sentence is the demo instruction for this payee. The payee, the amount, and the error code are from the transaction."
    : story.injectionFrom === "chain-memo"
      ? "The sentence is the demo scenario named in the memo. The payee, the amount, and the error code are from the transaction."
      : "The payee, the amount, and the error code are from the transaction. I did not have a stored narrative.";
  return {
    txHash: hash,
    atMs: null,
    kind,
    injection: story.injection,
    injectionFrom: story.injectionFrom,
    reasoning: "Rebuilt from the chain. This is the pay call in the transaction, not a stored model trace.",
    reasoningFrom: "chain",
    agentMode: "chain",
    payee: pay.payee,
    amountDisplay,
    errorCode: code,
    ruleText,
    explorerUrl: (config.explorerTxBase || "") + hash,
    checks: [
      { name: "Reads the finding", result: story.injection },
      { name: "Injection", result: pay.payee || "The payee is in the transaction." },
      { name: "Signs pay", result: amountDisplay || "The amount is in the transaction." },
      { name: "Contract", result: code || (pay.held ? "Held for the owner" : kind === "settled" ? "Settled" : "Refused") },
    ],
    source: "chain",
    note: payeeNote,
  };
}

async function fetchOne(rpcUrl, hash, signers) {
  for (const signer of signers.filter(Boolean)) {
    try {
      const status = await historyRpc(rpcUrl, "EXPERIMENTAL_tx_status", {
        tx_hash: hash,
        sender_account_id: signer,
        wait_until: "NONE",
      });
      const pay = readPayTransaction(status);
      if (pay) return pay;
    } catch {
      // The next signer may be the one that sent it.
    }
  }
  return null;
}

export async function fetchPayTransaction(rpcUrl, hash, signers) {
  if (!hash) return null;
  if (resolved.has(hash)) return resolved.get(hash);
  if (inflight.has(hash)) return inflight.get(hash);
  const job = fetchOne(rpcUrl, hash, signers).then((value) => {
    inflight.delete(hash);
    if (value) resolved.set(hash, value);
    return value;
  });
  inflight.set(hash, job);
  return job;
}

export function fillRow(row, pay, decimals) {
  if (!pay) return row;
  let amountDisplay = row.amount || null;
  if (!amountDisplay && pay.amount) {
    try {
      amountDisplay = formatAtomic(pay.amount, decimals);
    } catch {
      amountDisplay = null;
    }
  }
  return {
    ...row,
    payee: row.payee || pay.payee || null,
    amount: amountDisplay,
    errorCode: row.errorCode || pay.errorCode || null,
    outcome: row.outcome || (pay.held ? "held" : pay.outcome),
  };
}

export async function enrichRecent(config, rows) {
  const signers = [config.accounts.agent, config.accounts.owner];
  const hashes = [...new Set((rows || []).map((row) => row.txHash).filter(Boolean))].slice(0, 8);
  await Promise.all(hashes.map((hash) => fetchPayTransaction(config.rpcUrl, hash, signers).catch(() => null)));
  return (rows || []).map((row) => fillRow(row, resolved.get(row.txHash) || null, config.policy.decimals));
}

export async function chainReplay(config, hash) {
  const pay = await fetchPayTransaction(config.rpcUrl, hash, [config.accounts.agent, config.accounts.owner]);
  if (!pay) return null;
  return replayFromChain(config, hash, pay);
}

export function isTrapRefusal(pay, honeypot) {
  if (!pay || !honeypot) return false;
  if (String(pay.payee || "").toLowerCase() !== String(honeypot).toLowerCase()) return false;
  if (pay.outcome === "success" && !pay.errorCode && !pay.held) return false;
  return true;
}

export function isAttackerRefusal(pay, allowlist) {
  if (!pay || !pay.payee) return false;
  if (pay.outcome === "success" && !pay.errorCode && !pay.held) return false;
  const payee = String(pay.payee).toLowerCase();
  const allowed = (allowlist || []).map((item) => String(item).toLowerCase());
  return !allowed.includes(payee);
}

export async function honeypotHitsFromChain(config, honeypot) {
  if (!config.accounts.agent) return [];
  const page = await fetchIndexerPage(config.indexerAccountsBase, config.accounts.agent, { limit: 25 });
  const pays = (page.rows || []).filter((row) => (row.methods || []).includes("pay") && row.txHash);
  const signers = [config.accounts.agent, config.accounts.owner];
  await Promise.all(pays.map((row) => fetchPayTransaction(config.rpcUrl, row.txHash, signers).catch(() => null)));
  const allowlist = config.policy && config.policy.payees ? config.policy.payees : [];
  const feed = [];
  for (const row of pays) {
    const pay = resolved.get(row.txHash);
    if (!pay) continue;
    const trap = honeypot && isTrapRefusal(pay, honeypot);
    if (!trap && !isAttackerRefusal(pay, allowlist)) continue;
    const story = narrativeForPay(config, pay);
    let amountDisplay = null;
    if (pay.amount) {
      try {
        amountDisplay = formatAtomic(pay.amount, config.policy.decimals);
      } catch {
        amountDisplay = null;
      }
    }
    feed.push({
      atMs: row.atMs || null,
      payee: pay.payee,
      amount: amountDisplay,
      outcome: pay.held ? "held" : "refused",
      errorCode: pay.errorCode,
      txHash: row.txHash,
      explorerUrl: (config.explorerTxBase || "") + row.txHash,
      prompt: story.injection,
      trap: Boolean(trap),
      source: "chain",
    });
  }
  return feed;
}
