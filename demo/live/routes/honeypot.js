import { loadConfig } from "../lib/config.js";
import { json } from "../lib/http.js";
import { honeypotAccount, listHoneypot } from "../lib/honeypot.js";
import { honeypotHitsFromChain } from "../lib/txread.js";

export async function GET() {
  const config = loadConfig();
  const payee = honeypotAccount(config) || null;
  const local = await listHoneypot();
  let chain = [];
  try {
    chain = await honeypotHitsFromChain(config, payee);
  } catch {
    chain = [];
  }
  const byHash = new Map();
  for (const row of [...chain, ...(local.feed || [])]) {
    if (!row || !row.txHash || byHash.has(row.txHash)) continue;
    byHash.set(row.txHash, row);
  }
  const feed = [...byHash.values()];
  const fromChain = chain.length > 0;
  return json(200, {
    source: fromChain ? "chain" : local.source,
    onChainRegistry: false,
    payee,
    note: fromChain
      ? "These are pay calls to the trap account, read from the agent's transactions and the RPC. Each link is the refusal on NEAR. The deployed wallet does not keep a separate attacker registry."
      : local.note,
    feed,
  });
}
