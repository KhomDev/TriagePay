// Public demo configuration, plus the two signing keys. Keys come from
// the environment. A gitignored .keys/ file is only a local convenience,
// and only when VERCEL is unset.

import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DemoError } from "./http.js";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CONFIG_PATH = join(ROOT, "config.json");

const ACCOUNT_ENV = {
  TRIAGEPAY_OWNER_ACCOUNT: "owner",
  TRIAGEPAY_AGENT_ACCOUNT: "agent",
  TRIAGEPAY_WALLET_ACCOUNT: "wallet",
  TRIAGEPAY_TOKEN_ACCOUNT: "token",
  TRIAGEPAY_PAYEE_ACCOUNT: "payee",
  TRIAGEPAY_INJECTED_ACCOUNT: "injected",
  TRIAGEPAY_FLAGGED_ACCOUNT: "flagged",
};

const KEY_RE = /^ed25519:[1-9A-HJ-NP-Za-km-z]+$/;

export function assertPublicConfig(config) {
  const dumped = JSON.stringify(config);
  if (/ed25519:[1-9A-HJ-NP-Za-km-z]{20,}/.test(dumped)) {
    throw new Error("public config contains a signing key");
  }
  if (/"secret_key"\s*:/.test(dumped)) {
    throw new Error("public config contains secret_key");
  }
}

function numberOr(value, fallback) {
  if (value == null || value === "") return fallback;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

export function applyEnv(config, env) {
  const next = structuredClone(config);
  assertPublicConfig(next);
  if (env.TRIAGEPAY_RPC_URL) next.rpcUrl = env.TRIAGEPAY_RPC_URL;
  for (const [name, field] of Object.entries(ACCOUNT_ENV)) {
    if (env[name]) next.accounts[field] = env[name];
  }
  next.cooldownSeconds = Math.max(0, numberOr(env.TRIAGEPAY_COOLDOWN_SECONDS, next.cooldownSeconds));
  next.maxAttemptsPerDay = Math.max(0, numberOr(env.TRIAGEPAY_MAX_ATTEMPTS_PER_DAY, next.maxAttemptsPerDay));
  next.ipLimit = Math.max(1, numberOr(env.TRIAGEPAY_IP_LIMIT, next.ipLimit || 4));
  next.ipWindowSeconds = Math.max(1, numberOr(env.TRIAGEPAY_IP_WINDOW_SECONDS, next.ipWindowSeconds || 60));
  if ((!next.policy.payees || next.policy.payees.length === 0) && next.accounts.payee) {
    next.policy.payees = [next.accounts.payee];
  }
  if ((!next.flaggedAccounts || next.flaggedAccounts.length === 0) && next.accounts.flagged) {
    next.flaggedAccounts = [next.accounts.flagged];
  }
  return next;
}

export function readPublicConfig() {
  return JSON.parse(readFileSync(CONFIG_PATH, "utf8"));
}

export function loadConfig(env = process.env) {
  return applyEnv(readPublicConfig(), env);
}

export function chainReady(config) {
  const accounts = config.accounts || {};
  return Boolean(config.ready && accounts.wallet && accounts.agent && accounts.owner && accounts.payee && accounts.token);
}

function keyFile(role) {
  return join(ROOT, ".keys", `${role}.json`);
}

function readKeyFile(role) {
  const file = keyFile(role);
  if (!existsSync(file)) return "";
  const parsed = JSON.parse(readFileSync(file, "utf8"));
  return String(parsed.secret_key || "").trim();
}

function normalizeKey(value, name) {
  const trimmed = String(value || "").trim();
  if (!trimmed) return null;
  if (!KEY_RE.test(trimmed)) {
    throw new DemoError("bad_key", `${name} is not an ed25519 secret key`, 500);
  }
  return trimmed;
}

export function loadKeys(env = process.env) {
  const local = !env.VERCEL;
  const agent = (env.TRIAGEPAY_AGENT_SECRET_KEY || "").trim() || (local ? readKeyFile("agent") : "");
  const owner = (env.TRIAGEPAY_OWNER_SECRET_KEY || "").trim() || (local ? readKeyFile("owner") : "");
  return {
    agent: normalizeKey(agent, "TRIAGEPAY_AGENT_SECRET_KEY"),
    owner: normalizeKey(owner, "TRIAGEPAY_OWNER_SECRET_KEY"),
  };
}

export function signingReady(keys) {
  return Boolean(keys && keys.agent && keys.owner);
}
