// Public evidence that a payment tried the honeypot payee and the wallet
// did not settle it. The refusal transaction is the on-chain record. This
// list only points at those transactions. The deployed wallet has no
// separate attacker registry.

import { storeFromEnv } from "./store.js";
import { redact } from "./http.js";

const memory = [];

export function resetHoneypotForTests() {
  memory.length = 0;
}

export function honeypotAccount(config, env = process.env) {
  const fromEnv = String(env.TRIAGEPAY_HONEYPOT_ACCOUNT || "").trim().toLowerCase();
  return fromEnv || String((config && config.accounts && config.accounts.injected) || "").toLowerCase();
}

function rowFrom(input) {
  return {
    atMs: input.atMs || Date.now(),
    payee: input.payee,
    outcome: input.outcome || "refused",
    errorCode: input.errorCode || null,
    txHash: input.txHash || null,
    explorerUrl: input.explorerUrl || null,
    prompt: redact(String(input.prompt || "")).replace(/\s+/g, " ").trim().slice(0, 160),
  };
}

export async function recordHoneypot(input, env = process.env) {
  if (!input || !input.payee || !input.txHash) return null;
  if (input.outcome === "settled") {
    // A settle is not evidence of a trap working. Keep it visible anyway.
  }
  const row = rowFrom(input);
  memory.unshift(row);
  if (memory.length > 20) memory.length = 20;
  const store = storeFromEnv(env);
  if (store) {
    try {
      await store.lpush("triagepay:honeypot", JSON.stringify(row));
      await store.ltrim("triagepay:honeypot", 0, 19);
      return { ...row, source: "store" };
    } catch {
      return { ...row, source: "memory" };
    }
  }
  return { ...row, source: "memory" };
}

export async function listHoneypot(env = process.env) {
  const store = storeFromEnv(env);
  if (store) {
    try {
      const rows = await store.lrange("triagepay:honeypot", 0, 19);
      const feed = [];
      for (const raw of rows) {
        try {
          feed.push(JSON.parse(raw));
        } catch {
          // Skip a corrupt row.
        }
      }
      return {
        source: "store",
        onChainRegistry: false,
        note: "Each link is a transaction on NEAR. This list is stored beside those transactions. The deployed wallet does not keep its own attacker registry.",
        feed,
      };
    } catch {
      // Fall through.
    }
  }
  return {
    source: memory.length ? "memory" : "none",
    onChainRegistry: false,
    note: memory.length
      ? "Each link is a transaction on NEAR. This list is only what this instance has seen. The deployed wallet does not keep its own attacker registry."
      : "No honeypot attempt is stored on this instance yet. A refusal of the trap payee is the on-chain evidence, and this list points at those transactions when this server sees them.",
    feed: memory.slice(),
  };
}
