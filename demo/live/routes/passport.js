import { chainReady, loadConfig } from "../lib/config.js";
import { DemoError, fromError, json } from "../lib/http.js";
import { isNearAccount } from "../lib/accounts.js";
import { tokenSymbol } from "../lib/format.js";
import { buildPassport } from "../lib/passport.js";
import { recall } from "../lib/journal.js";
import { fetchIndexerPage } from "../lib/facts.js";
import { cachedPay, fetchPayTransaction } from "../lib/txread.js";
import { formatAtomic } from "../lib/format.js";
import { readReview } from "../lib/demo.js";
import { viewCall } from "../lib/near.js";
import { renderReceiptPng } from "../lib/og-image.js";

async function listPassportTxns(config, accountId) {
  const rows = [];
  let cursor = null;
  for (let page = 0; page < 4; page += 1) {
    const batch = await fetchIndexerPage(config.indexerAccountsBase, accountId, { limit: 25, next: cursor });
    rows.push(...(batch.rows || []));
    if (!batch.next) break;
    cursor = batch.next;
  }
  const wallet = await fetchIndexerPage(config.indexerAccountsBase, config.accounts.wallet, { limit: 25 });
  for (const row of wallet.rows || []) {
    const methods = row.methods || [];
    if (methods.includes("approve_payment") || methods.includes("reject_payment")) rows.push(row);
  }
  const pays = rows.filter((row) => (row.methods || []).includes("pay") && row.txHash).slice(0, 12);
  await Promise.all(pays.map((row) => fetchPayTransaction(
    config.rpcUrl,
    row.txHash,
    [accountId, config.accounts.agent, config.accounts.owner],
  ).catch(() => null)));
  const decoded = pays.map((row) => {
    const pay = cachedPay(row.txHash);
    if (!pay) return null;
    let amount = null;
    try {
      amount = pay.amount ? formatAtomic(pay.amount, config.policy.decimals) : null;
    } catch {
      amount = null;
    }
    return {
      txHash: row.txHash,
      atMs: row.atMs,
      payee: pay.payee,
      amount,
      errorCode: pay.errorCode,
    };
  }).filter(Boolean);
  return { rows, decoded };
}

function accountFrom(request) {
  const url = new URL(request.url);
  return String(url.searchParams.get("account") || "").trim().toLowerCase();
}

export async function GET(request) {
  try {
    const config = loadConfig();
    const accountId = accountFrom(request) || config.accounts.agent;
    if (!isNearAccount(accountId)) throw new DemoError("bad_account", "That is not a NEAR account id.", 400);
    if (!chainReady(config)) {
      return json(200, buildPassport({
        accountId,
        wallet: config.accounts.wallet,
        agent: null,
        review: null,
        indexed: [],
        journal: [],
        explorerTx: config.explorerTxBase,
        explorerAccount: config.explorerAccountBase,
        decimals: config.policy.decimals,
        symbol: tokenSymbol(config.accounts.token),
      }));
    }
    const [agent, review, history] = await Promise.all([
      viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", { agent_id: accountId }),
      readReview(config),
      listPassportTxns(config, accountId),
    ]);
    return json(200, buildPassport({
      accountId,
      wallet: config.accounts.wallet,
      agent,
      review,
      indexed: history.rows,
      journal: [...(history.decoded || []), ...recall()],
      explorerTx: config.explorerTxBase,
      explorerAccount: config.explorerAccountBase,
      decimals: config.policy.decimals,
      symbol: tokenSymbol(config.accounts.token),
    }));
  } catch (error) {
    return fromError(error);
  }
}

export async function og(request) {
  const config = loadConfig();
  const accountId = accountFrom(request) || config.accounts.agent || "agent";
  const png = await renderReceiptPng({
    kind: "settled",
    headline: "PASSPORT",
    rule: "AGENT",
    ruleText: "Limits and history, read from the wallet.",
    payee: "",
    amount: "",
    tx: "",
    tried: isNearAccount(accountId) ? `Agent ${accountId}` : "Agent passport",
  });
  return new Response(png, { status: 200, headers: { "content-type": "image/png", "cache-control": "public, max-age=300" } });
}
