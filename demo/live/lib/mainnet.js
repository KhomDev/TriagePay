// Mainnet is optional. The testnet demo is the default. This module only
// signs when TRIAGEPAY_MAINNET_AGENT_SECRET_KEY is set, and then only a
// 1000-yocto pay to the allowlisted gateway plus one refused attacker pay.

import { fetchDayStamps, fetchIndexerPage } from "./facts.js";
import { formatAmount } from "./format.js";
import { DemoError } from "./http.js";
import { clientIp } from "./limits.js";
import { functionCall, rpc, signCall } from "./near.js";
import { storeFromEnv } from "./store.js";
import { fetchPayTransaction } from "./txread.js";
import { createHash } from "node:crypto";

export const MAINNET_PAID_AMOUNT = "1000";
const IP_WINDOW_MS = 10 * 60 * 1000;
const GLOBAL_RUNS_PER_DAY = 30;
const TXS_PER_RUN = 2;

export const MAINNET = {
  network: "mainnet",
  rpcUrl: "https://rpc.mainnet.fastnear.com",
  indexerAccountsBase: "https://api.nearblocks.io/v3/accounts/",
  explorerTxBase: "https://nearblocks.io/txns/",
  explorerAccountBase: "https://nearblocks.io/address/",
  accounts: {
    owner: "hlpok1lad.tg",
    agent: "tp-agent2.hlpok1lad.tg",
    wallet: "tp-wallet.hlpok1lad.tg",
    token: "wrap.near",
    payee: "gateway.hlpok1lad.tg",
    injected: "tp-mallory.hlpok1lad.tg",
  },
  policy: {
    decimals: 24,
    payees: ["gateway.hlpok1lad.tg"],
  },
};

export const MAINNET_PROOFS = [
  { label: "Paid call", href: "https://nearblocks.io/txns/2RH77VLsLHovF3m4kzR76jk71X8tYKAyugxQ9YJMXLJJ" },
  { label: "Refused payee", href: "https://nearblocks.io/txns/HskzPv94J1JY16K5WdUG8wfGznnhnMMKcXgE1acks63F" },
  { label: "Key revoked", href: "https://nearblocks.io/txns/CFutaZM9XMLBBcfm2jwH2JnQQJA8ReNGyTRhH1PZLzFh" },
];

const KEY_RE = /^ed25519:[1-9A-HJ-NP-Za-km-z]+$/;
const ipHits = new Map();

export function resetMainnetLimits() {
  ipHits.clear();
}

export function mainnetAgentKey(env = process.env) {
  const raw = String(env.TRIAGEPAY_MAINNET_AGENT_SECRET_KEY || "").trim();
  if (!raw || !KEY_RE.test(raw)) return null;
  return raw;
}

export function mainnetStatus(env = process.env) {
  return {
    signing: Boolean(mainnetAgentKey(env)),
    wallet: MAINNET.accounts.wallet,
    agent: MAINNET.accounts.agent,
    payee: MAINNET.accounts.payee,
    injected: MAINNET.accounts.injected,
    paidAmount: MAINNET_PAID_AMOUNT,
    proofs: MAINNET_PROOFS,
  };
}

export function tallyPays(rows) {
  const pays = (rows || []).filter((row) => (row.methods || []).includes("pay") && row.txHash);
  const signers = new Set();
  let paid = 0;
  let refused = 0;
  for (const row of pays) {
    if (row.signer) signers.add(row.signer);
    if (row.success) paid += 1;
    else refused += 1;
  }
  return { paid, refused, signers: [...signers], sampled: pays.length };
}

function ipSlot(ip, nowMs) {
  const bucket = Math.floor(nowMs / IP_WINDOW_MS);
  const hash = createHash("sha256").update(String(ip || "unknown")).digest("hex").slice(0, 24);
  return { bucket, key: `triagepay:mainnet:ip:${hash}:${bucket}` };
}

function takeLocalIp(ip, nowMs) {
  const recent = (ipHits.get(ip) || []).filter((stamp) => nowMs - stamp < IP_WINDOW_MS);
  if (recent.length >= 1) {
    ipHits.set(ip, recent);
    return false;
  }
  recent.push(nowMs);
  ipHits.set(ip, recent);
  return true;
}

export async function reserveMainnetRun({ request, nowMs = Date.now(), chainPayCount = 0, store = null }) {
  if (chainPayCount >= GLOBAL_RUNS_PER_DAY * TXS_PER_RUN) {
    return { allowed: false, reason: "Mainnet has used its 30 runs for today." };
  }
  const ip = clientIp(request);
  if (!takeLocalIp(ip, nowMs)) {
    return { allowed: false, reason: "This address already ran mainnet in the last 10 minutes." };
  }
  const day = new Date(nowMs).toISOString().slice(0, 10);
  const dayKey = `triagepay:mainnet:runs:${day}`;
  if (!store) return { allowed: true, source: "instance" };
  const slot = ipSlot(ip, nowMs);
  try {
    const ipCount = await store.incr(slot.key);
    if (ipCount === 1) await store.expire(slot.key, 600);
    if (ipCount > 1) {
      return { allowed: false, reason: "This address already ran mainnet in the last 10 minutes." };
    }
    const runs = await store.incr(dayKey);
    if (runs === 1) await store.expire(dayKey, 60 * 60 * 30);
    if (runs > GLOBAL_RUNS_PER_DAY) {
      await store.decr(dayKey);
      return { allowed: false, reason: "Mainnet has used its 30 runs for today." };
    }
    return { allowed: true, source: "store" };
  } catch {
    return { allowed: true, source: "instance" };
  }
}

async function assertMainnet(rpcUrl) {
  const status = await rpc(rpcUrl, "status", []);
  if (!status || status.chain_id !== "mainnet") {
    throw new DemoError("not_mainnet", "The RPC endpoint is not NEAR mainnet.", 500);
  }
}

function chainView(signed, label) {
  return {
    submitted: Boolean(signed && signed.txHash && !signed.rejected),
    txHash: signed && signed.txHash ? signed.txHash : null,
    explorerUrl: signed && signed.txHash ? MAINNET.explorerTxBase + signed.txHash : null,
    outcome: signed ? signed.outcome : null,
    errorCode: signed ? signed.errorCode : null,
    rejected: Boolean(signed && signed.rejected),
    label,
  };
}

async function signPay(secretKey, { payee, amount, memo }) {
  const action = await functionCall("pay", {
    token: MAINNET.accounts.token,
    receiver_id: payee,
    amount,
    memo,
  }, 50, 0n);
  return signCall({
    rpcUrl: MAINNET.rpcUrl,
    signerId: MAINNET.accounts.agent,
    secretKey,
    receiverId: MAINNET.accounts.wallet,
    actions: [action],
  });
}

export async function runMainnetLaunch(request, env = process.env) {
  const secretKey = mainnetAgentKey(env);
  if (!secretKey) {
    throw new DemoError("mainnet_off", "Mainnet signing is off. The proof links are the record.", 409);
  }
  await assertMainnet(MAINNET.rpcUrl);
  const day = await fetchDayStamps(MAINNET.indexerAccountsBase, MAINNET.accounts.agent, Date.now());
  const gate = await reserveMainnetRun({
    request,
    chainPayCount: (day.stamps || []).length,
    store: storeFromEnv(env),
  });
  if (!gate.allowed) throw new DemoError("mainnet_limit", gate.reason, 429);
  const refusedSigned = await signPay(secretKey, {
    payee: MAINNET.accounts.injected,
    amount: MAINNET_PAID_AMOUNT,
    memo: "demo:mainnet-injected",
  });
  const paidSigned = await signPay(secretKey, {
    payee: MAINNET.accounts.payee,
    amount: MAINNET_PAID_AMOUNT,
    memo: "demo:mainnet-paid",
  });
  const chain = chainView(refusedSigned, "refused");
  const paid = chainView(paidSigned, "paid");
  return {
    network: "mainnet",
    agent: { mode: "scripted", label: "Scripted", reasoning: "Scripted. The agent pays the injected account, then the allowlisted gateway.", tool: { name: "pay", receiver_id: MAINNET.accounts.injected, amount: MAINNET_PAID_AMOUNT }, fallbackReason: "scripted", model: null },
    chain,
    paid,
    summary: "Mainnet: the injected payee was sent first, then 1000 yocto wNEAR to the allowlisted gateway.",
    receipt: chain.txHash ? {
      kind: chain.errorCode || chain.outcome !== "success" ? "refused" : "settled",
      headline: chain.errorCode || chain.outcome !== "success" ? "REFUSED" : "SETTLED",
      rule: chain.errorCode || "",
      ruleText: "The payee is not on the allowlist.",
      tried: "The agent pays the injected account.",
      amount: "1000 yocto wNEAR",
      payee: MAINNET.accounts.injected,
      txHash: chain.txHash,
      explorerUrl: chain.explorerUrl,
      shareText: "REFUSED. The payee is not on the allowlist. built at NEAR HACKATHON @NEARProtocol #NEARHackathon",
    } : null,
  };
}

const TESTNET_INDEX = {
  network: "testnet",
  rpcUrl: "https://test.rpc.fastnear.com",
  indexerAccountsBase: "https://api-testnet.nearblocks.io/v3/accounts/",
  agent: "tpd-e947e7-agent.testnet",
};

let scoreboardCache = { at: 0, value: null };

export async function scoreboard() {
  if (scoreboardCache.value && Date.now() - scoreboardCache.at < 60_000) return scoreboardCache.value;
  const sides = [
    { ...TESTNET_INDEX },
    { network: "mainnet", rpcUrl: MAINNET.rpcUrl, indexerAccountsBase: MAINNET.indexerAccountsBase, agent: MAINNET.accounts.agent },
  ];
  let paid = 0;
  let refused = 0;
  const signers = new Set();
  let protectedYocto = 0n;
  let decoded = 0;
  let truncated = false;
  const notes = [];
  for (const side of sides) {
    const page = await fetchIndexerPage(side.indexerAccountsBase, side.agent, { limit: 25 });
    if (!page.ok) {
      notes.push(`${side.network} index did not answer`);
      continue;
    }
    if (page.next) truncated = true;
    const tally = tallyPays(page.rows);
    paid += tally.paid;
    refused += tally.refused;
    for (const signer of tally.signers) signers.add(signer);
    const refusedRows = (page.rows || []).filter((row) => (row.methods || []).includes("pay") && row.txHash && !row.success).slice(0, 4);
    for (const row of refusedRows) {
      try {
        const pay = await fetchPayTransaction(side.rpcUrl, row.txHash, [side.agent]);
        if (pay && pay.amount) {
          protectedYocto += BigInt(pay.amount);
          decoded += 1;
        }
      } catch {
        // This call stays in the refused count. Its amount is not added.
      }
    }
  }
  const value = {
    paid,
    refused,
    agents: signers.size,
    protectedDisplay: decoded ? formatAmount(protectedYocto, 24, "wNEAR") : null,
    protectedDecoded: decoded,
    usdc: "0 USDC",
    truncated,
    source: notes.length
      ? `NearBlocks pay calls for the testnet and mainnet agents. ${notes.join(". ")}.`
      : "NearBlocks pay calls for tpd-e947e7-agent.testnet and tp-agent2.hlpok1lad.tg. One page each. Refused amounts are from the calls I decoded. USDC is 0 because these wallets pay wNEAR.",
  };
  scoreboardCache = { at: Date.now(), value };
  return value;
}
