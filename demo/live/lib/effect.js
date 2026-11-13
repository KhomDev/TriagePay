// What `pay` will do, from the same order as the wallet contract:
// pause, caps, then hold-or-transfer. A challenge uses this so I do not
// sign a payment the wallet would settle.

export function predictEffect({ payee, amount, agent, review, walletPaused = false, tokenAllowed = true }) {
  if (walletPaused) return { effect: "refuse", code: "ERR_PAUSED" };
  if (!tokenAllowed) return { effect: "refuse", code: "ERR_TOKEN_NOT_ALLOWED" };
  if (!agent || !agent.active) return { effect: "refuse", code: "ERR_AGENT_INACTIVE" };
  let amt;
  try {
    amt = BigInt(amount);
  } catch {
    return { effect: "refuse", code: "bad_amount" };
  }
  if (amt <= 0n) return { effect: "refuse", code: "ERR_ZERO_AMOUNT" };
  const per = BigInt(agent.per_call_max || "0");
  const daily = BigInt(agent.daily_max || "0");
  const spent = BigInt(agent.spent_in_window || "0");
  const allowlisted = Array.isArray(agent.payees) && agent.payees.map(String).includes(String(payee));
  const threshold = BigInt((review && review.approvalThreshold) || "0");
  const reviewOn = Boolean(review && review.supported !== false && review.reviewUnknownPayees);
  const holdNew = reviewOn && !allowlisted;
  const holdAmount = allowlisted && threshold > 0n && amt > threshold;
  if (amt > per) return { effect: "refuse", code: "ERR_PER_CALL_LIMIT" };
  if (spent + amt > daily) return { effect: "refuse", code: "ERR_DAILY_LIMIT" };
  const pending = Number(review && review.pendingCount) || 0;
  const maxPending = Number(review && review.maxPending) || 0;
  if ((holdNew || holdAmount) && maxPending > 0 && pending >= maxPending) {
    return { effect: "refuse", code: "ERR_REVIEW_QUEUE_FULL" };
  }
  if (holdNew || holdAmount) {
    return { effect: "hold", code: holdNew ? "HELD_NEW_PAYEE" : "HELD_ABOVE_THRESHOLD" };
  }
  if (!allowlisted) return { effect: "refuse", code: "ERR_PAYEE_NOT_ALLOWED" };
  return { effect: "settle", code: null };
}
