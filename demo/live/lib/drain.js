// Drain Me log and per-address cap. The shared store keeps the log across
// instances. Without it, this process keeps the log in memory and the
// journal file. Signing still goes through the daily cap in limits.js.

import { createHash } from "node:crypto";
import { clientIp } from "./limits.js";
import { memoryStore, storeFromEnv } from "./store.js";
import { DemoError, redact } from "./http.js";

const local = memoryStore();
const localHits = new Map();

export function resetDrainForTests() {
  localHits.clear();
}

export function cleanPrompt(text) {
  if (typeof text !== "string") return "";
  return text.replace(/\u0000/g, "").trim().slice(0, 500);
}

export function redactPrompt(text) {
  const clean = redact(cleanPrompt(text))
    .replace(/\b[0-9a-f]{64}\b/gi, "[redacted-hex]")
    .replace(/\s+/g, " ")
    .trim();
  if (clean.length <= 180) return clean;
  const cut = clean.slice(0, 180);
  const space = cut.lastIndexOf(" ");
  return `${(space > 40 ? cut.slice(0, space) : cut).trim()}…`;
}

function drainLimit(env = process.env) {
  const limit = Number(env.TRIAGEPAY_DRAIN_IP_LIMIT || 3);
  const windowSeconds = Number(env.TRIAGEPAY_DRAIN_WINDOW_SECONDS || 600);
  return {
    limit: Number.isFinite(limit) ? Math.max(1, Math.min(30, limit)) : 3,
    windowMs: (Number.isFinite(windowSeconds) ? Math.max(10, Math.min(86400, windowSeconds)) : 600) * 1000,
  };
}

function ipSlot(ip, bucket) {
  const hash = createHash("sha256").update(String(ip)).digest("hex").slice(0, 24);
  return `triagepay:drain:ip:${hash}:${bucket}`;
}

export async function assertDrainRate(request, env = process.env, nowMs = Date.now()) {
  const { limit, windowMs } = drainLimit(env);
  const ip = clientIp(request);
  const store = storeFromEnv(env);
  if (!store) {
    const key = ip || "unknown";
    const recent = (localHits.get(key) || []).filter((stamp) => nowMs - stamp < windowMs);
    if (recent.length >= limit) {
      localHits.set(key, recent);
      const oldest = recent[0];
      throw new DemoError(
        "ip_rate_limit",
        `This address has used its ${limit} Drain Me attempts for this window. Try again in ${Math.max(1, Math.ceil((windowMs - (nowMs - oldest)) / 1000))} seconds.`,
        429,
        { retry_after_seconds: Math.max(1, Math.ceil((windowMs - (nowMs - oldest)) / 1000)) },
      );
    }
    recent.push(nowMs);
    localHits.set(key, recent);
    return { source: "memory" };
  }
  const bucket = Math.floor(nowMs / windowMs);
  const key = ipSlot(ip, bucket);
  try {
    const count = await store.incr(key);
    if (count === 1) await store.expire(key, Math.ceil(windowMs / 1000) + 5);
    if (count > limit) {
      throw new DemoError(
        "ip_rate_limit",
        `This address has used its ${limit} Drain Me attempts for this window.`,
        429,
        { retry_after_seconds: Math.ceil(windowMs / 1000) },
      );
    }
    return { source: "store" };
  } catch (error) {
    if (error instanceof DemoError) throw error;
    throw new DemoError(
      "limit_store_unavailable",
      "The shared attempt store is configured and did not answer, so this server did not run Drain Me.",
      503,
    );
  }
}

function safeEntry(entry) {
  return {
    atMs: entry.atMs || Date.now(),
    prompt: redactPrompt(entry.prompt || ""),
    payee: entry.payee || null,
    amount: entry.amount || null,
    amountDisplay: entry.amountDisplay || null,
    outcome: entry.outcome || "no_payment",
    errorCode: entry.errorCode || null,
    txHash: entry.txHash || null,
    explorerUrl: entry.explorerUrl || null,
    mode: entry.mode === "live" ? "live" : "scripted",
  };
}

async function bump(store, entry) {
  await store.incr("triagepay:drain:attempts");
  if (entry.outcome === "refused") await store.incr("triagepay:drain:refusals");
  if (entry.outcome === "settled" && entry.amount) {
    const current = BigInt((await store.get("triagepay:drain:drained")) || "0");
    await store.set("triagepay:drain:drained", (current + BigInt(entry.amount)).toString());
  }
}

export async function recordDrain(entry, env = process.env) {
  const safe = safeEntry(entry);
  await local.lpush("triagepay:drain:memory", JSON.stringify(safe));
  await local.ltrim("triagepay:drain:memory", 0, 19);
  const attempts = Number((await local.get("triagepay:drain:attempts")) || 0) + 1;
  await local.set("triagepay:drain:attempts", String(attempts));
  if (safe.outcome === "refused") {
    const refusals = Number((await local.get("triagepay:drain:refusals")) || 0) + 1;
    await local.set("triagepay:drain:refusals", String(refusals));
  }
  if (safe.outcome === "settled" && safe.amount) {
    const drained = BigInt((await local.get("triagepay:drain:drained")) || "0") + BigInt(safe.amount);
    await local.set("triagepay:drain:drained", drained.toString());
  }
  const store = storeFromEnv(env);
  if (store) {
    try {
      await store.lpush("triagepay:drain:log", JSON.stringify(safe));
      await store.ltrim("triagepay:drain:log", 0, 19);
      await bump(store, safe);
    } catch {
      // The memory log still has the row. The status call reports the source.
    }
  }
  return safe;
}

function parseRows(rows) {
  const out = [];
  for (const row of rows || []) {
    try {
      const parsed = JSON.parse(row);
      if (parsed && typeof parsed === "object") out.push(safeEntry(parsed));
    } catch {
      // Skip a corrupt row rather than failing the page.
    }
  }
  return out;
}

export async function drainStatus(env = process.env) {
  const store = storeFromEnv(env);
  if (store) {
    try {
      const [attempts, refusals, drained, rows] = await Promise.all([
        store.get("triagepay:drain:attempts"),
        store.get("triagepay:drain:refusals"),
        store.get("triagepay:drain:drained"),
        store.lrange("triagepay:drain:log", 0, 19),
      ]);
      return {
        source: "store",
        attempts: Number(attempts || 0),
        refusals: Number(refusals || 0),
        drained: String(drained || "0"),
        feed: parseRows(rows),
        note: "Try to make the agent pay an attacker. The contract is what stops it.",
      };
    } catch {
      // Fall through to this process.
    }
  }
  const rows = parseRows(await local.lrange("triagepay:drain:memory", 0, 19));
  return {
    source: rows.length ? "memory" : "none",
    attempts: Number((await local.get("triagepay:drain:attempts")) || rows.length),
    refusals: Number((await local.get("triagepay:drain:refusals")) || rows.filter((row) => row.outcome === "refused").length),
    drained: String((await local.get("triagepay:drain:drained")) || "0"),
    feed: rows,
    note: "Try to make the agent pay an attacker. The contract is what stops it.",
  };
}
