// Replay records for a refusal or a hold. The chain transaction is the
// evidence. The narrative (the prompt, the reasoning) lives here, in the
// shared store when one is set, otherwise in this process.

import { storeFromEnv } from "./store.js";
import { redact } from "./http.js";

const memory = new Map();
const MAX = 40;

export function resetReplayForTests() {
  memory.clear();
}

function clip(text, max) {
  return redact(String(text || "")).replace(/\s+/g, " ").trim().slice(0, max);
}

export function replayRecord(input) {
  const tx = String(input.txHash || "");
  if (!/^[A-Za-z0-9]{8,88}$/.test(tx)) return null;
  const kind = input.held ? "held" : input.outcome === "success" && !input.errorCode ? "settled" : "refused";
  return {
    txHash: tx,
    atMs: input.atMs || Date.now(),
    kind,
    injection: clip(input.injection, 500),
    reasoning: clip(input.reasoning, 700),
    agentMode: input.agentMode === "live" ? "live" : "scripted",
    payee: input.payee || null,
    amountDisplay: input.amountDisplay || null,
    errorCode: input.errorCode || null,
    ruleText: input.ruleText || null,
    explorerUrl: input.explorerUrl || null,
    checks: Array.isArray(input.checks) ? input.checks.slice(0, 6).map((item) => ({
      name: clip(item.name, 40),
      result: clip(item.result, 500),
    })) : [],
    source: "record",
  };
}

export async function saveReplay(input, env = process.env) {
  const record = replayRecord(input);
  if (!record) return null;
  memory.set(record.txHash, record);
  const store = storeFromEnv(env);
  if (store) {
    try {
      await store.set(`triagepay:replay:${record.txHash}`, JSON.stringify(record));
    } catch {
      record.source = "memory";
    }
  } else {
    record.source = "memory";
  }
  return record;
}

export async function loadReplay(txHash, env = process.env) {
  const tx = String(txHash || "");
  if (!/^[A-Za-z0-9]{8,88}$/.test(tx)) return null;
  const store = storeFromEnv(env);
  if (store) {
    try {
      const raw = await store.get(`triagepay:replay:${tx}`);
      if (raw) {
        const parsed = JSON.parse(raw);
        return { ...parsed, source: "store" };
      }
    } catch {
      // Fall through to this process.
    }
  }
  const local = memory.get(tx);
  return local ? { ...local, source: "memory" } : null;
}

export function partialReplay(txHash, explorerBase) {
  const tx = String(txHash || "");
  if (!/^[A-Za-z0-9]{8,88}$/.test(tx)) return null;
  return {
    txHash: tx,
    atMs: null,
    kind: "unknown",
    injection: "",
    reasoning: "",
    agentMode: null,
    payee: null,
    amountDisplay: null,
    errorCode: null,
    ruleText: null,
    explorerUrl: explorerBase ? explorerBase + tx : null,
    checks: [],
    source: "chain-link-only",
    note: "This server does not have the agent's text for that transaction. The explorer link is the chain record.",
  };
}
