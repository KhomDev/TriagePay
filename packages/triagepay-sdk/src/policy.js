// The one call an agent is allowed to make: pay() on the policy wallet,
// with no deposit. The wallet enforces the allowlist and the limits.

const GAS = "50000000000000";

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
  const note = memo == null ? null : String(memo).slice(0, 256);
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

/** A payer for the x402 helper. `sign(call)` returns the transaction hash. */
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
