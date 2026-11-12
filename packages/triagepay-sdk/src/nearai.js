// Tool definition for a NEAR AI style agent. The model may ask to pay.
// The only thing `execute` can submit is `pay` on the policy wallet.

import { policyPayCall } from "./policy.js";

export function nearAiPaymentTool({ wallet, token, sign }) {
  if (typeof sign !== "function") throw new Error("sign(call) must return a transaction hash");
  return {
    type: "function",
    function: {
      name: "triagepay_pay",
      description: "Pay a payee through the TriagePay policy wallet. The wallet enforces the allowlist, the per-call cap, and the daily cap. This tool cannot withdraw or change policy.",
      parameters: {
        type: "object",
        additionalProperties: false,
        properties: {
          receiver_id: { type: "string", description: "NEAR account to pay." },
          amount: { type: "string", description: "Integer token amount in atomic units." },
          memo: { type: "string", description: "Optional memo, at most 256 bytes." },
        },
        required: ["receiver_id", "amount"],
      },
    },
    async execute(args) {
      const call = policyPayCall({
        wallet,
        token,
        receiverId: args && args.receiver_id,
        amount: args && args.amount,
        memo: args && args.memo,
      });
      const txHash = await sign(call);
      return { tx_hash: txHash ? String(txHash) : null, call };
    },
  };
}
