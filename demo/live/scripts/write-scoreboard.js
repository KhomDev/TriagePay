// Rebuild public/scoreboard.json from the NearBlocks index. A partial
// walk does not replace the file. Totals never go down.

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { utcDayStartMs } from "../lib/cooldown.js";
import { fetchIndexerPage } from "../lib/facts.js";
import {
  MAX_PAGES,
  PAGE_LIMIT,
  SIDES,
  amountFromRpcStatus,
  amountFromTxn,
  presentScoreboard,
  raiseScoreboard,
  sumProtected,
  readSnapshot,
  takeAll,
  tallyPaySet,
} from "../lib/scoreboard.js";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "scoreboard.json");

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function allPages(side) {
  const pages = [];
  let cursor = null;
  for (let i = 0; i < MAX_PAGES; i += 1) {
    let page = { ok: false, rows: [], next: null };
    for (let attempt = 0; attempt < 4; attempt += 1) {
      page = await fetchIndexerPage(side.indexerAccountsBase, side.agent, { limit: PAGE_LIMIT, next: cursor });
      if (page.ok) break;
      await sleep(400 * (attempt + 1));
    }
    pages.push(page);
    if (!page.ok || !page.next) break;
    cursor = page.next;
  }
  return pages;
}

async function amountFor(side, hash) {
  try {
    const rpc = await fetch(side.rpcUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "scoreboard",
        method: "EXPERIMENTAL_tx_status",
        params: { tx_hash: hash, sender_account_id: side.agent, wait_until: "NONE" },
      }),
      signal: AbortSignal.timeout(8000),
    });
    if (rpc.ok) {
      const body = await rpc.json();
      const amount = amountFromRpcStatus(body);
      if (amount) return { amount, answered: true };
      if (body && body.result && body.result.transaction) return { amount: null, answered: true };
    }
  } catch {
    // The index detail is the second source.
  }
  try {
    const response = await fetch(side.txBase + hash, {
      headers: { accept: "application/json" },
      signal: AbortSignal.timeout(8000),
    });
    if (!response.ok) return { amount: null, answered: false };
    return { amount: amountFromTxn(await response.json()), answered: true };
  } catch {
    return { amount: null, answered: false };
  }
}

const nowMs = Date.now();
const previous = readSnapshot() || {};
const known = previous.refusedAmounts && typeof previous.refusedAmounts === "object" ? previous.refusedAmounts : {};
const day = new Date(nowMs).toISOString().slice(0, 10);
const networks = {};
const rowsBySide = {};
let incomplete = false;

for (const side of SIDES) {
  const pages = await allPages(side);
  const collected = takeAll(pages, MAX_PAGES);
  if (!collected.complete) {
    console.error(`${side.network} index did not finish inside ${MAX_PAGES} pages`);
    incomplete = true;
    break;
  }
  rowsBySide[side.network] = collected.rows;
  const newest = tallyPaySet(collected.rows).newest;
  networks[side.network] = {
    agent: side.agent,
    newestHash: newest ? newest.hash : null,
    newestAtMs: newest ? newest.atMs : null,
    transactions: collected.rows.length,
  };
  console.error(`${side.network} ${collected.rows.length} transactions`);
}

if (incomplete) process.exit(0);

const testnet = tallyPaySet(rowsBySide.testnet, utcDayStartMs(nowMs));
const mainnet = tallyPaySet(rowsBySide.mainnet, null);
const refusedHashes = [
  ...testnet.refusedHashes.map((hash) => ({ side: SIDES[0], hash })),
  ...mainnet.refusedHashes.map((hash) => ({ side: SIDES[1], hash })),
];
const amounts = {};
let amountsOk = true;
for (const item of refusedHashes) {
  const cached = typeof known[item.hash] === "string" ? known[item.hash] : null;
  if (cached) {
    amounts[item.hash] = cached;
    continue;
  }
  const found = await amountFor(item.side, item.hash);
  if (!found.answered) {
    amountsOk = false;
    console.error(`missing amount ${item.hash}`);
    continue;
  }
  if (found.amount) amounts[item.hash] = found.amount;
  await sleep(80);
}

const protectedYocto = BigInt(sumProtected(amounts));
if (!amountsOk) {
  const kept = {
    ...previous,
    refusedAmounts: { ...known, ...amounts },
  };
  writeFileSync(OUT, `${JSON.stringify(kept, null, 2)}\n`);
  console.error("A refused amount is missing. Saved the amounts that did resolve.");
  process.exit(0);
}

const fresh = {
  version: 1,
  definition: "Pay calls by tpd-e947e7-agent.testnet and tp-e2e-agent.hlpok1lad.tg, deduped by transaction hash. Paid means the transaction succeeded. Refused means it did not. Value protected sums refused amounts up to 1000 wNEAR each.",
  day,
  asOf: new Date(nowMs).toISOString(),
  paid: testnet.paid + mainnet.paid,
  refused: testnet.refused + mainnet.refused,
  agentIds: [...new Set([...testnet.agentIds, ...mainnet.agentIds])].sort(),
  protectedYocto: protectedYocto.toString(),
  attemptsToday: testnet.attemptsToday,
  maxAttemptsPerDay: 300,
  networks,
  refusedAmounts: amounts,
  complete: true,
  truncated: false,
  sourceUrl: "https://nearblocks.io",
};
const raised = raiseScoreboard(previous, fresh);
const presented = {
  ...presentScoreboard({ ...fresh, ...raised, networks, version: 1 }, nowMs),
  refusedAmounts: amounts,
  version: 1,
};
writeFileSync(OUT, `${JSON.stringify(presented, null, 2)}\n`);
console.log(JSON.stringify({
  paid: presented.paid,
  refused: presented.refused,
  agents: presented.agents,
  protectedDisplay: presented.protectedDisplay,
  attemptsRemaining: presented.attemptsRemaining,
  asOf: presented.asOf,
}));
