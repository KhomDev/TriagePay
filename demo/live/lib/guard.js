// Request guards for the hosted demo (red-team 2026-11-13).
//
// - ownerAuthorized: owner-only actions (applying a policy, approving a
//   payment to a payee outside the committed allowlist) need a bearer
//   token the public page never has.
// - assertModelBudget: every outbound NEAR AI call is counted before it is
//   made, per address and per day, so the public routes cannot be used as
//   a free proxy to the operator's key.

import { createHash, timingSafeEqual } from "node:crypto";
import { DemoError } from "./http.js";
import { clientIp } from "./limits.js";
import { storeFromEnv } from "./store.js";

function digest(value) {
  return createHash("sha256").update(String(value)).digest();
}

/** True when the request carries the operator's owner-action token. */
export function ownerAuthorized(request, env = process.env) {
  const expected = String(env.TRIAGEPAY_OWNER_ACTION_TOKEN || "").trim();
  if (expected.length < 16) return false;
  const header = request && request.headers ? request.headers.get("authorization") || "" : "";
  const match = /^Bearer\s+(.+)$/i.exec(header.trim());
  if (!match) return false;
  return timingSafeEqual(digest(match[1].trim()), digest(expected));
}

const modelHits = new Map();
let modelDay = { day: "", count: 0 };

export function resetModelBudget() {
  modelHits.clear();
  modelDay = { day: "", count: 0 };
}

function modelLimits(env) {
  const perIp = Number(env.TRIAGEPAY_MODEL_CALLS_PER_IP || 6);
  const perDay = Number(env.TRIAGEPAY_MODEL_CALLS_PER_DAY || 300);
  return {
    perIp: Number.isFinite(perIp) && perIp > 0 ? perIp : 6,
    perDay: Number.isFinite(perDay) && perDay > 0 ? perDay : 300,
    windowMs: 60_000,
  };
}

function refuse(code, message, retry) {
  return new DemoError(code, message, 429, { retry_after_seconds: retry });
}

/**
 * Count one outbound model call before it is made. Throws 429 when the
 * caller's address or the day is over budget. With a shared store the
 * counters are global; without one they are per instance.
 */
export async function assertModelBudget(request, env = process.env, nowMs = Date.now()) {
  const { perIp, perDay, windowMs } = modelLimits(env);
  const ip = clientIp(request);
  const day = new Date(nowMs).toISOString().slice(0, 10);
  const store = storeFromEnv(env);
  if (store) {
    const ipHash = createHash("sha256").update(String(ip)).digest("hex").slice(0, 24);
    const bucket = Math.floor(nowMs / windowMs);
    const ipKey = `triagepay:model-ip:${ipHash}:${bucket}`;
    const dayKey = `triagepay:model-day:${day}`;
    let ipCount;
    let dayCount;
    try {
      ipCount = await store.incr(ipKey);
      if (ipCount === 1) await store.expire(ipKey, Math.ceil(windowMs / 1000) + 5);
      dayCount = await store.incr(dayKey);
      if (dayCount === 1) await store.expire(dayKey, 26 * 3600);
    } catch {
      throw new DemoError("limit_store_unavailable", "The shared store did not answer, so I did not call the model.", 503, { retry_after_seconds: 15 });
    }
    if (ipCount > perIp) throw refuse("model_rate_limit", `This address has used its ${perIp} model calls for this minute.`, 60);
    if (dayCount > perDay) throw refuse("model_daily_cap", "The demo has used today's model budget. The deterministic path still works.", 3600);
    return { source: "shared" };
  }
  if (modelDay.day !== day) modelDay = { day, count: 0 };
  if (modelDay.count >= perDay) {
    throw refuse("model_daily_cap", "The demo has used today's model budget. The deterministic path still works.", 3600);
  }
  const key = ip || "unknown";
  const recent = (modelHits.get(key) || []).filter((stamp) => nowMs - stamp < windowMs);
  if (recent.length >= perIp) {
    modelHits.set(key, recent);
    throw refuse("model_rate_limit", `This address has used its ${perIp} model calls for this minute.`, Math.max(1, Math.ceil((windowMs - (nowMs - recent[0])) / 1000)));
  }
  recent.push(nowMs);
  modelHits.set(key, recent);
  modelDay.count += 1;
  return { source: "instance" };
}
