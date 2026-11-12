// Pre-payment payee classification. Pure: the caller supplies chain facts.
// The wallet contract remains the backstop. This layer explains the payee
// before the agent key is asked to sign.

export const EMPTY_CODE_HASH = "11111111111111111111111111111111";

const ORDER = [
  "account_missing",
  "flagged",
  "not_allowlisted",
  "unverified_contract",
  "newly_created",
  "age_unknown",
];

const SEVERITY = {
  account_missing: "block",
  flagged: "block",
  not_allowlisted: "block",
  unverified_contract: "block",
  newly_created: "warn",
  age_unknown: "warn",
  allowlisted: "allow",
  zero_amount: "block",
  per_call_limit: "block",
  daily_limit: "block",
  within_limit: "allow",
};

function reason(code, text) {
  return { code, severity: SEVERITY[code], text };
}

function worst(reasons) {
  if (reasons.some((item) => item.severity === "block")) return "block";
  if (reasons.some((item) => item.severity === "warn")) return "warn";
  return "allow";
}

/**
 * @param {object} facts
 * @param {string} facts.accountId
 * @param {boolean} facts.exists
 * @param {string|null} facts.codeHash base58 code hash from view_account
 * @param {number|null} facts.createdAtMs
 * @param {number} facts.nowMs
 * @param {string[]} facts.allowlist
 * @param {string[]} facts.flagged
 * @param {string[]} facts.verifiedCodeHashes
 * @param {number} [facts.newAccountWindowMs]
 */
export function classifyPayee(facts) {
  const accountId = String(facts.accountId || "");
  const allow = new Set((facts.allowlist || []).map(String));
  const flagged = new Set((facts.flagged || []).map(String));
  const verified = new Set((facts.verifiedCodeHashes || []).map(String));
  const windowMs = Number.isFinite(facts.newAccountWindowMs) ? facts.newAccountWindowMs : 604800000;
  const codes = [];

  if (!facts.exists) codes.push("account_missing");
  if (flagged.has(accountId)) codes.push("flagged");
  if (!allow.has(accountId)) codes.push("not_allowlisted");

  const hash = facts.codeHash || null;
  const hasCode = Boolean(hash) && hash !== EMPTY_CODE_HASH;
  if (facts.exists && hasCode && !verified.has(hash)) codes.push("unverified_contract");

  if (facts.exists) {
    if (facts.createdAtMs == null || facts.nowMs == null) {
      codes.push("age_unknown");
    } else {
      const age = facts.nowMs - facts.createdAtMs;
      if (!Number.isFinite(age) || age < 0) codes.push("age_unknown");
      else if (age < windowMs) codes.push("newly_created");
    }
  }

  codes.sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
  const days = Math.max(1, Math.round(windowMs / 86400000));
  const reasons = codes.map((code) => {
    if (code === "newly_created") return reason(code, `newly created account (under ${days} days)`);
    if (code === "account_missing") return reason(code, "account does not exist");
    if (code === "flagged") return reason(code, "account is on the flagged list");
    if (code === "not_allowlisted") return reason(code, "not on the payee allowlist");
    if (code === "unverified_contract") return reason(code, "unknown or unverified contract");
    return reason(code, "account age could not be confirmed");
  });
  const verdict = reasons.length === 0 ? "allow" : worst(reasons);
  if (verdict === "allow") {
    reasons.push(reason("allowlisted", "on the payee allowlist"));
  }
  return { verdict, reasons };
}

/**
 * Amount check against the same limits the contract enforces.
 * `spentToday` and `dailyMax` are optional; omitted means the daily
 * total is left to the chain.
 */
export function classifyAmount(facts) {
  const amount = BigInt(facts.amount);
  const perCall = BigInt(facts.perCallMax);
  const reasons = [];
  if (amount <= 0n) reasons.push(reason("zero_amount", "amount is zero"));
  if (amount > perCall) reasons.push(reason("per_call_limit", "amount is above the per-call limit"));
  if (facts.spentToday != null && facts.dailyMax != null) {
    const spent = BigInt(facts.spentToday);
    const daily = BigInt(facts.dailyMax);
    if (amount > 0n && spent + amount > daily) {
      reasons.push(reason("daily_limit", "amount would exceed the daily limit"));
    }
  }
  if (reasons.length === 0) reasons.push(reason("within_limit", "within the per-call and daily limits"));
  return { verdict: worst(reasons), reasons };
}
