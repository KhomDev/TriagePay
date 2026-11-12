// Global cooldown for signed demo attempts. In-memory state is not enough
// on a serverless platform, so the caller also passes timestamps read from
// the chain index. The contract daily limit and the key allowance remain
// the hard caps.

export function utcDayStartMs(nowMs) {
  const date = new Date(nowMs);
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate());
}

export function assessCooldown({ nowMs, timestampsMs, cooldownMs, maxPerDay }) {
  const stamps = (timestampsMs || []).filter((value) => Number.isFinite(value));
  const dayStart = utcDayStartMs(nowMs);
  const attemptsToday = stamps.filter((value) => value >= dayStart && value <= nowMs + 1000).length;
  const latest = stamps.reduce((max, value) => (value > max ? value : max), 0);
  const elapsed = latest ? nowMs - latest : Infinity;
  if (maxPerDay >= 0 && attemptsToday >= maxPerDay) {
    const next = dayStart + 24 * 60 * 60 * 1000;
    return {
      allowed: false,
      code: "daily_attempt_cap",
      retryAfterSeconds: Math.max(1, Math.ceil((next - nowMs) / 1000)),
      attemptsToday,
    };
  }
  if (cooldownMs > 0 && elapsed < cooldownMs) {
    return {
      allowed: false,
      code: "cooldown",
      retryAfterSeconds: Math.max(1, Math.ceil((cooldownMs - elapsed) / 1000)),
      attemptsToday,
    };
  }
  return { allowed: true, code: null, retryAfterSeconds: 0, attemptsToday };
}

export function cooldownMessage(decision, cooldownSeconds, maxPerDay) {
  if (decision.allowed) return null;
  if (decision.code === "daily_attempt_cap") {
    return `This demo has used its ${maxPerDay} signed attempts for the UTC day. Try again in ${decision.retryAfterSeconds} seconds.`;
  }
  return `This demo signs at most once every ${cooldownSeconds} seconds so the testnet account is not drained. Try again in ${decision.retryAfterSeconds} seconds.`;
}
