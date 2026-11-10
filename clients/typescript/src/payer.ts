// Reference payer: signs the planned call with near-api-js 7.3.1.
//
// The key file is JSON {"account_id": "...", "secret_key": "ed25519:..."}
// kept outside any repository. The secret is never printed.

import { readFileSync } from "node:fs";
import type { Payer, Proof, Requirement } from "./x402.ts";
import { DEFAULT_RPC, planPayment, type Mode } from "./actions.ts";

export type PayerOptions = {
  mode: Mode;
  keyFile: string;
  account?: string;
  wallet?: string;
  rpcUrl?: string;
  log?: (line: string) => void;
};

function loadKey(keyFile: string, account?: string): { accountId: string; secretKey: string } {
  const raw = JSON.parse(readFileSync(keyFile, "utf8"));
  const accountId = account ?? raw.account_id;
  const secretKey = raw.secret_key ?? raw.private_key;
  if (!accountId) throw new Error("no account id (pass --account or put account_id in the key file)");
  if (typeof secretKey !== "string" || !secretKey.startsWith("ed25519:")) {
    throw new Error("the key file has no ed25519 secret_key");
  }
  return { accountId, secretKey };
}

/** Sign and send the payment; resolve once the transaction executed.
 *  The server waits for FINAL itself and answers settlement_pending
 *  until then. */
export async function sendPayment(requirement: Requirement, options: PayerOptions): Promise<Proof> {
  const near: any = await import("near-api-js");
  const { accountId, secretKey } = loadKey(options.keyFile, options.account);
  const plan = planPayment(requirement, options.mode, options.wallet);
  const url = options.rpcUrl ?? DEFAULT_RPC[requirement.network];
  const provider = new near.JsonRpcProvider({ url });
  const account = new near.Account(accountId, provider, secretKey);
  options.log?.(`paying ${requirement.amount} of ${requirement.asset} to ${requirement.payTo} via ${plan.methodName} on ${plan.receiverId}`);
  const result: any = await account.signAndSendTransaction({
    receiverId: plan.receiverId,
    actions: [near.actions.functionCall(plan.methodName, plan.args, plan.gas, plan.deposit)],
    waitUntil: "EXECUTED_OPTIMISTIC",
    throwOnFailure: false,
  });
  const txHash: string = result?.transaction?.hash ?? result?.transaction_outcome?.id;
  if (!txHash) throw new Error("the node returned no transaction hash");
  if (result?.status?.Failure) {
    throw new Error(`the payment transaction failed: ${JSON.stringify(result.status.Failure).slice(0, 300)} (tx ${txHash})`);
  }
  return { tx_hash: txHash, signer_id: accountId };
}

export function nearPayer(options: PayerOptions): Payer {
  return (requirement) => sendPayment(requirement, options);
}
