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
      ? "Refused payments to accounts that are not on the allowlist."
      : "No refused attacker payment is on the chain index yet.",
    feed,
  });
}
