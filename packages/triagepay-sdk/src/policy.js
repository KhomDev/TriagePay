// The one call an agent is allowed to make: pay() on the policy wallet,
// with no deposit. The wallet enforces the allowlist and the limits.

const GAS = "50000000000000";
const MEMO_BYTES = 256;

/** Keep a memo inside the wallet's 256-byte limit without splitting a code point. */
export function clipMemo(memo) {
  if (memo == null) return null;
  const text = String(memo);
  const bytes = Buffer.from(text, "utf8");
  if (bytes.length <= MEMO_BYTES) return text;
  let end = MEMO_BYTES;
  while (end > 0 && (bytes[end] & 0xc0) === 0x80) end -= 1;
  return bytes.subarray(0, end).toString("utf8");
}

/**
 * Build the only call an agent key may submit: `pay` on `wallet`, deposit 0.
 * `amount` is a positive integer in the token's atomic units. A memo is
 * clipped to 256 UTF-8 bytes, which is the contract's limit.
 */
export function policyPayCall({ wallet, token, receiverId, amount, memo }) {
  if (!wallet || !token || !receiverId) {
    throw new Error("wallet, token, and receiverId are required");
  }
  let units;
  try {
    units = BigInt(amount);
  } catch {
    throw new Error("amount must be an integer in atomic units");
  }
  if (units <= 0n) throw new Error("amount must be positive");
  const note = clipMemo(memo);
  return {
    receiverId: wallet,
    methodName: "pay",
    args: {
      token,
      receiver_id: receiverId,
      amount: units.toString(),
      memo: note,
    },
    gas: GAS,
    deposit: "0",
  };
}

/**
 * A payer for `payPerCall`. `sign(call)` submits the pay call and returns
 * the transaction hash. The returned function turns an x402 requirement
 * into `{ tx_hash, signer_id }`.
 */
export function createPolicyPayer({ wallet, token, signerId, sign }) {
  if (!signerId) throw new Error("signerId is required");
  if (typeof sign !== "function") throw new Error("sign(call) must return a transaction hash");
  return async function pay(requirement) {
    const call = policyPayCall({
      wallet,
      token: token || requirement.asset,
      receiverId: requirement.payTo,
      amount: requirement.amount,
      memo: requirement.extra && requirement.extra.memo,
    });
    const txHash = await sign(call);
    if (!txHash) throw new Error("sign did not return a transaction hash");
    return { tx_hash: String(txHash), signer_id: signerId };
  };
}
