import { chainReady, loadConfig } from "../lib/config.js";
import { DemoError, fromError, json } from "../lib/http.js";
import { isNearAccount } from "../lib/accounts.js";
import { tokenSymbol } from "../lib/format.js";
import { buildPassport } from "../lib/passport.js";
import { recall } from "../lib/journal.js";
import { fetchIndexerTxns } from "../lib/facts.js";
import { readReview } from "../lib/demo.js";
import { viewCall } from "../lib/near.js";
import { renderReceiptPng } from "../lib/og-image.js";

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
    const [agent, review, indexed] = await Promise.all([
      viewCall(config.rpcUrl, config.accounts.wallet, "get_agent", { agent_id: accountId }),
      readReview(config),
      fetchIndexerTxns(config.indexerAccountsBase, accountId),
    ]);
    return json(200, buildPassport({
      accountId,
      wallet: config.accounts.wallet,
      agent,
      review,
      indexed,
      journal: recall(),
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
    kind: "refused",
    headline: "PASSPORT",
    rule: "AGENT",
    ruleText: "Limits and history, read from the wallet.",
    payee: isNearAccount(accountId) ? accountId : "",
    amount: "",
    tx: "",
    tried: "Agent passport",
  });
  return new Response(png, { status: 200, headers: { "content-type": "image/png", "cache-control": "public, max-age=300" } });
}
