// One-link revoke. HMAC, an expiry, and a nonce. The server still signs
// with the owner key. Single-use is exact when the shared store is set.
// Without it, only this process remembers the nonce.

import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { memoryStore, storeFromEnv } from "./store.js";

const local = memoryStore();

export function killSecret(env = process.env) {
  return String(env.KILL_SWITCH_SECRET || "").trim();
}

export function killTtlMs(env = process.env) {
  const seconds = Number(env.KILL_SWITCH_TTL_SECONDS || 600);
  const bounded = Number.isFinite(seconds) ? Math.min(3600, Math.max(30, seconds)) : 600;
  return bounded * 1000;
}

export function mintKillToken({ secret, nowMs, ttlMs, nonce }) {
  if (!secret) throw new Error("missing secret");
  const id = nonce || randomBytes(16).toString("hex");
  const exp = nowMs + ttlMs;
  const body = `${exp}.${id}`;
  const mac = createHmac("sha256", secret).update(body).digest("base64url");
  return { token: `${body}.${mac}`, exp, nonce: id };
}

export function verifyKillToken(token, secret, nowMs) {
  const parts = String(token || "").split(".");
  if (parts.length !== 3) return { ok: false, reason: "malformed" };
  const [exp, nonce, mac] = parts;
  if (!/^\d{1,16}$/.test(exp) || !/^[a-f0-9]{16,64}$/.test(nonce)) return { ok: false, reason: "malformed" };
  if (!secret) return { ok: false, reason: "no_secret" };
  const expected = createHmac("sha256", secret).update(`${exp}.${nonce}`).digest("base64url");
  const left = Buffer.from(mac);
  const right = Buffer.from(expected);
  if (left.length !== right.length || !timingSafeEqual(left, right)) return { ok: false, reason: "bad_signature" };
  if (BigInt(exp) < BigInt(nowMs)) return { ok: false, reason: "expired" };
  return { ok: true, exp: Number(exp), nonce };
}

function nonceKey(nonce) {
  return `triagepay:kill:${nonce}`;
}

export async function consumeNonce(nonce, ttlMs, env = process.env, store = storeFromEnv(env)) {
  const key = nonceKey(nonce);
  const target = store || local;
  const got = await target.setNxPx(key, "1", Math.max(1000, ttlMs));
  return { ok: got === "OK", source: store ? "store" : "memory", store: target, key };
}

export async function releaseNonce(claimed) {
  if (!claimed || !claimed.store || !claimed.key) return;
  try {
    await claimed.store.del(claimed.key);
  } catch {
    // A failed revoke can be retried until the nonce expires on its own.
  }
}

export function publicOrigin(request) {
  const forwardedHost = request.headers.get("x-forwarded-host") || request.headers.get("host") || "";
  const host = forwardedHost.split(",")[0].trim();
  if (/^[a-z0-9.-]+(?::\d+)?$/i.test(host)) {
    const proto = (request.headers.get("x-forwarded-proto") || "").split(",")[0].trim();
    const scheme = proto === "http" ? "http" : "https";
    return `${scheme}://${host}`;
  }
  return new URL(request.url).origin;
}
