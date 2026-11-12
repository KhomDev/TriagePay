// Cross-instance attempt budget. A shared store (Vercel KV or Upstash)
// holds the counters with INCR, so two serverless instances cannot both
// pass the cap. The chain index is the backstop for the daily total and
// the fallback when no store is configured. Per-IP limiting needs the
// store; without it, only this process remembers the address.

import { createHash } from "node:crypto";
import { utcDayStartMs } from "./cooldown.js";

export const DEFAULT_IP_LIMIT = 4;
export const DEFAULT_IP_WINDOW_MS = 60_000;
const LOCK_MS = 25_000;
const localHits = new Map();

export function resetLocalLimits() {
  localHits.clear();
}

export function takeLocalIp(ip, nowMs, limit = DEFAULT_IP_LIMIT, windowMs = DEFAULT_IP_WINDOW_MS) {
  const key = ip || "unknown";
  const recent = (localHits.get(key) || []).filter((stamp) => nowMs - stamp < windowMs);
  if (recent.length >= limit) {
    localHits.set(key, recent);
    const oldest = recent[0];
    return {
      allowed: false,
      code: "ip_rate_limit",
      retryAfterSeconds: Math.max(1, Math.ceil((windowMs - (nowMs - oldest)) / 1000)),
      source: "instance",
    };
  }
  recent.push(nowMs);
  localHits.set(key, recent);
  return { allowed: true, code: null, source: "instance" };
}

export function clientIp(request) {
  if (!request || !request.headers || typeof request.headers.get !== "function") return "unknown";
  const real = request.headers.get("x-real-ip");
  if (real && real.trim()) return real.trim();
  const forwarded = request.headers.get("x-forwarded-for");
  if (forwarded && forwarded.trim()) return forwarded.split(",")[0].trim();
  return "unknown";
}

export function ipKey(ip, bucket) {
  const hash = createHash("sha256").update(String(ip)).digest("hex").slice(0, 24);
  return `triagepay:ip:${hash}:${bucket}`;
}

export function dayKey(nowMs) {
  return `triagepay:attempts:${new Date(nowMs).toISOString().slice(0, 10)}`;
}

function secondsUntilDayEnd(nowMs) {
  const next = utcDayStartMs(nowMs) + 24 * 60 * 60 * 1000;
  return Math.max(60, Math.ceil((next - nowMs) / 1000) + 3600);
}

async function rollback(store, keys) {
  for (const key of keys) {
    if (key.op === "decr") await store.decr(key.key);
    if (key.op === "del") await store.del(key.key);
  }
}

/**
 * Reserve one signed attempt.
 * `chain` is the result of assessCooldown over indexed timestamps, or null.
 * When `store` is set and errors, this throws so the caller fails closed.
 */
export async function reserveAttempt({
  store,
  nowMs,
  ip,
  maxPerDay,
  cooldownMs,
  ipLimit = DEFAULT_IP_LIMIT,
  ipWindowMs = DEFAULT_IP_WINDOW_MS,
  chain = null,
  lockToken,
}) {
  if (chain && !chain.allowed) {
    return {
      allowed: false,
      code: chain.code,
      retryAfterSeconds: chain.retryAfterSeconds,
      attemptsToday: chain.attemptsToday,
      source: store ? "shared+chain" : "chain",
    };
  }
  if (!store) {
    if (!chain) {
      return {
        allowed: false,
        code: "limit_unavailable",
        retryAfterSeconds: 30,
        attemptsToday: 0,
        source: "none",
      };
    }
    const ipDecision = takeLocalIp(ip, nowMs, ipLimit, ipWindowMs);
    if (!ipDecision.allowed) {
      return {
        ...ipDecision,
        attemptsToday: chain.attemptsToday,
      };
    }
    return { ...chain, source: "chain" };
  }

  const day = dayKey(nowMs);
  const bucket = Math.floor(nowMs / ipWindowMs);
  const ipSlot = ipKey(ip || "unknown", bucket);
  const cool = "triagepay:cooldown";
  const lock = "triagepay:signlock";
  const token = lockToken || `${nowMs}`;
  const gotLock = await store.setNxPx(lock, token, LOCK_MS);
  if (gotLock !== "OK") {
    const ttl = await store.pttl(lock);
    return {
      allowed: false,
      code: "cooldown",
      retryAfterSeconds: Math.max(1, Math.ceil((ttl > 0 ? ttl : 2000) / 1000)),
      attemptsToday: chain ? chain.attemptsToday : 0,
      source: "shared",
    };
  }
  const undo = [{ op: "del", key: lock }];
  try {
    const ipCount = await store.incr(ipSlot);
    if (ipCount === 1) await store.expire(ipSlot, Math.ceil(ipWindowMs / 1000) + 5);
    undo.push({ op: "decr", key: ipSlot });
    if (ipCount > ipLimit) {
      await rollback(store, undo);
      return {
        allowed: false,
        code: "ip_rate_limit",
        retryAfterSeconds: Math.max(1, Math.ceil(ipWindowMs / 1000)),
        attemptsToday: chain ? chain.attemptsToday : 0,
        source: "shared",
      };
    }
    const dayCount = await store.incr(day);
    if (dayCount === 1) await store.expire(day, secondsUntilDayEnd(nowMs));
    undo.push({ op: "decr", key: day });
    const seenBefore = Math.max(chain ? chain.attemptsToday : 0, dayCount - 1);
    if (seenBefore >= maxPerDay) {
      await rollback(store, undo);
      const next = utcDayStartMs(nowMs) + 24 * 60 * 60 * 1000;
      return {
        allowed: false,
        code: "daily_attempt_cap",
        retryAfterSeconds: Math.max(1, Math.ceil((next - nowMs) / 1000)),
        attemptsToday: seenBefore,
        source: "shared",
      };
    }
    const attemptsToday = seenBefore + 1;
    if (cooldownMs > 0) {
      const cooled = await store.setNxPx(cool, String(nowMs), cooldownMs);
      if (cooled !== "OK") {
        await rollback(store, undo);
        const ttl = await store.pttl(cool);
        return {
          allowed: false,
          code: "cooldown",
          retryAfterSeconds: Math.max(1, Math.ceil((ttl > 0 ? ttl : cooldownMs) / 1000)),
          attemptsToday: dayCount - 1,
          source: "shared",
        };
      }
    }
    return {
      allowed: true,
      code: null,
      retryAfterSeconds: 0,
      attemptsToday,
      source: "shared",
      lockToken: token,
    };
  } catch (error) {
    try {
      await rollback(store, undo);
    } catch {
      // The original store error is the one the caller should see.
    }
    throw error;
  }
}

export function releaseLock(store, token) {
  if (!store || !token) return Promise.resolve();
  return store.get("triagepay:signlock").then((current) => {
    if (current === token) return store.del("triagepay:signlock");
    return 0;
  });
}
