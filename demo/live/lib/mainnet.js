// Mainnet is optional. The testnet demo is the default. This module only
// signs when TRIAGEPAY_MAINNET_AGENT_SECRET_KEY is set, and then only a
// 1000-yocto pay to the allowlisted gateway plus one refused attacker pay.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { fetchIndexerPage } from "./facts.js";
import { formatAmountShort } from "./format.js";
import { DemoError } from "./http.js";
import { clientIp } from "./limits.js";
import { functionCall, rpc, signCall } from "./near.js";
import { storeFromEnv } from "./store.js";
import { fetchPayTransaction } from "./txread.js";
import { createHash } from "node:crypto";

const PROOFS_PATH = join(dirname(fileURLToPath(import.meta.url)), "..", "mainnet-proofs.json");

export const MAINNET_PAID_AMOUNT = "1000";
const IP_WINDOW_MS = 10 * 60 * 1000;
const GLOBAL_RUNS_PER_DAY = 30;

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

export function loadProofFile() {
  const parsed = JSON.parse(readFileSync(PROOFS_PATH, "utf8"));
  const proofs = Array.isArray(parsed.proofs) ? parsed.proofs : [];
  return proofs
    .filter((item) => item && item.href && item.label && !/revok/i.test(item.label))
    .map((item) => ({
      label: String(item.label),
      href: String(item.href),
      memo: item.memo ? String(item.memo) : "",
      amount: item.amount ? String(item.amount) : "",
      kind: item.kind ? String(item.kind) : "",
    }));
}

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
    proofs: loadProofFile(),
    remaining: null,
    runs: null,
  };
}

export function countDemoMemos(pays) {
  let injected = 0;
  let paid = 0;
  for (const pay of pays || []) {
    if (pay.memo === "demo:mainnet-injected") injected += 1;
    if (pay.memo === "demo:mainnet-paid") paid += 1;
  }
  return { injected, paid, runs: Math.max(injected, paid) };
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

export async function reserveMainnetRun({ request, nowMs = Date.now(), demoRunCount = 0, store = null }) {
  if (demoRunCount >= GLOBAL_RUNS_PER_DAY) {
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
  const held = Boolean(signed && signed.held);
  const refused = Boolean(signed && (signed.errorCode || signed.outcome === "failure"));
  return {
    submitted: Boolean(signed && signed.txHash && !signed.rejected),
    txHash: signed && signed.txHash ? signed.txHash : null,
    explorerUrl: signed && signed.txHash ? MAINNET.explorerTxBase + signed.txHash : null,
    outcome: signed ? signed.outcome : null,
    errorCode: signed ? signed.errorCode : null,
    held,
    rejected: Boolean(signed && signed.rejected),
    label: held ? "held" : label,
    honest: held ? "held" : refused ? "refused" : "settled",
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

async function recentPays() {
  const seen = new Set();
  const rows = [];
  let cursor = null;
  for (let page = 0; page < 2; page += 1) {
    const batch = await fetchIndexerPage(MAINNET.indexerAccountsBase, MAINNET.accounts.agent, { limit: 25, next: cursor });
    if (!batch.ok) break;
    for (const row of batch.rows || []) {
      if (!row.txHash || seen.has(row.txHash)) continue;
      if (!(row.methods || []).includes("pay")) continue;
      seen.add(row.txHash);
      rows.push(row);
    }
    if (!batch.next) break;
    cursor = batch.next;
  }
  const decoded = [];
  for (const row of rows) {
    try {
      const pay = await fetchPayTransaction(MAINNET.rpcUrl, row.txHash, [MAINNET.accounts.agent]);
      if (pay) decoded.push({ ...pay, txHash: row.txHash, atMs: row.atMs });
    } catch {
      // Skip a call the RPC did not return.
    }
  }
  return decoded;
}

let usageCache = { at: 0, value: null };

export async function demoUsage() {
  if (usageCache.value && Date.now() - usageCache.at < 60_000) return usageCache.value;
  const pays = await recentPays();
  const counted = countDemoMemos(pays);
  const pinned = [];
  for (const memo of ["demo:mainnet-paid", "demo:mainnet-injected"]) {
    const found = pays.find((pay) => pay.memo === memo && pay.txHash);
    if (!found) continue;
    pinned.push({
      label: memo === "demo:mainnet-paid" ? "Paid call" : "Refused payee",
      href: MAINNET.explorerTxBase + found.txHash,
      memo,
      amount: found.amount || MAINNET_PAID_AMOUNT,
      kind: memo === "demo:mainnet-paid" ? "paid" : "refused",
      verified: true,
    });
  }
  const configured = loadProofFile();
  const proofs = pinned.length ? pinned : configured;
  const value = {
    runs: counted.runs,
    remaining: Math.max(0, GLOBAL_RUNS_PER_DAY - counted.runs),
    proofs,
  };
  usageCache = { at: Date.now(), value };
  return value;
}

export async function mainnetPublic(env = process.env) {
  const base = mainnetStatus(env);
  try {
    const usage = await demoUsage();
    return { ...base, runs: usage.runs, remaining: usage.remaining, proofs: usage.proofs };
  } catch {
    return base;
  }
}

export async function runMainnetLaunch(request, env = process.env) {
  const secretKey = mainnetAgentKey(env);
  if (!secretKey) {
    throw new DemoError("mainnet_off", "Mainnet signing is off. The proof links are the record.", 409);
  }
  await assertMainnet(MAINNET.rpcUrl);
  const usage = await demoUsage().catch(() => ({ runs: 0 }));
  const gate = await reserveMainnetRun({
    request,
    demoRunCount: usage.runs || 0,
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
  const malloryHeld = Boolean(chain.held);
  const headline = malloryHeld ? "HELD" : (chain.errorCode || chain.outcome !== "success" ? "REFUSED" : "SETTLED");
  const ruleText = malloryHeld
    ? "The wallet held the payment instead of refusing it."
    : "The payee is not on the allowlist.";
  usageCache = { at: 0, value: null };
  return {
    network: "mainnet",
    agent: { mode: "deterministic", label: "deterministic attack, real on-chain refusal", reasoning: "Deterministic attack on mainnet.", tool: { name: "pay", receiver_id: MAINNET.accounts.injected, amount: MAINNET_PAID_AMOUNT }, fallbackReason: null, model: null },
    chain,
    paid,
    summary: malloryHeld
      ? "Mainnet held the attacker payment instead of refusing it."
      : "Mainnet refused the injected payee, then sent 1000 yocto wNEAR to the allowlisted gateway.",
    receipt: chain.txHash ? {
      kind: malloryHeld ? "held" : (headline === "REFUSED" ? "refused" : "settled"),
      headline,
      rule: chain.errorCode || "",
      ruleText,
      tried: "The agent pays the injected account.",
      amount: "1000 yocto wNEAR",
      payee: MAINNET.accounts.injected,
      txHash: chain.txHash,
      explorerUrl: chain.explorerUrl,
      shareText: `${headline}. ${ruleText} built at NEAR HACKATHON @NEARProtocol #NEARHackathon`,
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
    const seen = new Set();
    const rows = [];
    let cursor = null;
    let ok = false;
    for (let pageNo = 0; pageNo < 2; pageNo += 1) {
      const page = await fetchIndexerPage(side.indexerAccountsBase, side.agent, { limit: 25, next: cursor });
      if (!page.ok) break;
      ok = true;
      for (const row of page.rows || []) {
        if (!row.txHash || seen.has(row.txHash)) continue;
        seen.add(row.txHash);
        rows.push(row);
      }
      if (!page.next) break;
      truncated = true;
      cursor = page.next;
    }
    if (!ok) {
      notes.push(`${side.network} index did not answer`);
      continue;
    }
    const tally = tallyPays(rows);
    paid += tally.paid;
    refused += tally.refused;
    for (const signer of tally.signers) signers.add(signer);
    const refusedRows = rows.filter((row) => (row.methods || []).includes("pay") && row.txHash && !row.success).slice(0, 4);
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
    protectedDisplay: decoded ? formatAmountShort(protectedYocto, 24, "wNEAR") : null,
    protectedDecoded: decoded,
    truncated,
    source: "Live from NearBlocks",
    sourceUrl: "https://nearblocks.io",
    note: notes.length ? notes.join(". ") : "",
  };
  scoreboardCache = { at: Date.now(), value };
  return value;
}
