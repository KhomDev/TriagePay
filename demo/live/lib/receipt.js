// Plain-language receipt for a wallet outcome. The share line is fixed
// so a card and a post say the same thing.

export const SHARE_TAIL = "built at NEAR HACKATHON @NEARProtocol #NEARHackathon";

export const RULE_TEXT = {
  ERR_PAUSED: "The wallet is paused.",
  ERR_AGENT_INACTIVE: "The agent is revoked.",
  ERR_TOKEN_NOT_ALLOWED: "That token is not allowed.",
  ERR_PAYEE_NOT_ALLOWED: "The payee is not on the allowlist.",
  ERR_ZERO_AMOUNT: "The amount is zero.",
  ERR_PER_CALL_LIMIT: "The amount is above the per-call cap.",
  ERR_DAILY_LIMIT: "The amount would pass the daily cap.",
  ERR_OVERFLOW: "The amount overflowed the policy math.",
  ERR_REVIEW_QUEUE_FULL: "The owner still has payments waiting.",
  ERR_DEPOSIT_NOT_ZERO: "Pay takes no deposit.",
  ERR_MEMO_TOO_LONG: "The memo is longer than 256 bytes.",
  ERR_INSUFFICIENT_GAS: "The call did not attach enough gas.",
  ERR_NOT_OWNER: "Only the owner can do that.",
  ERR_NOT_PENDING: "No payment is waiting with that id.",
  ERR_NOT_PAUSED: "The wallet is not paused.",
  ERR_SELF_ACCOUNT: "The wallet cannot be the payee.",
  ERR_TOO_MANY_TOKENS: "The token list is full.",
  HELD_FOR_OWNER: "The wallet queued the payment for the owner.",
  HELD_NEW_PAYEE: "The payee is new, so the wallet queued the payment.",
  HELD_ABOVE_THRESHOLD: "The amount is above the owner's threshold, so the wallet queued it.",
  APPROVED: "The owner approved the held payment.",
  REJECTED: "The owner rejected the held payment.",
  SETTLED: "The wallet settled the payment.",
};

const HEADLINES = {
  refused: "REFUSED",
  held: "HELD",
  approved: "APPROVED",
  rejected: "REJECTED",
  settled: "SETTLED",
};

export function buildReceipt({ kind, code, tried, amount, payee, txHash, explorerUrl }) {
  const headline = HEADLINES[kind] || "RECEIPT";
  const rule = code && code !== headline ? code : "";
  const ruleText = RULE_TEXT[code] || RULE_TEXT[headline] || "The wallet applied its spend policy.";
  const movement = amount && payee ? `${amount} to ${payee}` : (amount || payee || "");
  const head = [headline, rule].filter(Boolean).join(" ");
  const shareText = `${[head, movement].filter(Boolean).join(". ")}. ${SHARE_TAIL}`;
  return {
    kind,
    headline,
    rule,
    ruleText,
    tried: tried || "The agent asked the wallet to pay.",
    amount: amount || null,
    payee: payee || null,
    txHash: txHash || null,
    explorerUrl: explorerUrl || null,
    shareText,
  };
}

function presentAmount(text) {
  const value = String(text || "");
  if (!value) return null;
  const over = /^(\d+\.\d{0,6}?)0{8,}1$/.exec(value);
  if (over) {
    const head = over[1].replace(/\.$/, "") || "0";
    return `${head} wNEAR plus 1 yocto`;
  }
  if (/[A-Za-z]/.test(value)) return value;
  return `${value} wNEAR`;
}

export function receiptFromRun({ scenario, chain, action, amountDisplay, payee }) {
  if (!chain || !chain.submitted) return null;
  let kind = "refused";
  let code = chain.errorCode || null;
  if (chain.held) {
    kind = "held";
    code = code || "HELD_FOR_OWNER";
  } else if (action === "approve" && chain.outcome === "success") {
    kind = "approved";
    code = "APPROVED";
  } else if (action === "reject" && chain.outcome === "success") {
    kind = "rejected";
    code = "REJECTED";
  } else if (chain.outcome === "success" && !chain.errorCode) {
    kind = "settled";
    code = "SETTLED";
  }
  return buildReceipt({
    kind,
    code,
    tried: (scenario && (scenario.narrative || scenario.title)) || "The agent asked the wallet to pay.",
    amount: presentAmount(amountDisplay || (scenario && scenario.amountDisplay) || "") ,
    payee: payee || (scenario && scenario.payee) || null,
    txHash: chain.txHash || null,
    explorerUrl: chain.explorerUrl || null,
  });
}

export function matchPending(pending, payee, amount) {
  const rows = Array.isArray(pending) ? pending : [];
  let best = null;
  for (const row of rows) {
    if (payee && String(row.receiver_id) !== String(payee)) continue;
    if (amount && String(row.amount) !== String(amount)) continue;
    const id = String(row.payment_id ?? "");
    if (!/^\d+$/.test(id)) continue;
    if (best == null || BigInt(id) > BigInt(best)) best = id;
  }
  return best;
}
