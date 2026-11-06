// Pure construction of the payment transaction for a requirement (no
// network, no keys), so the exact call a payer signs can be tested.

import type { Requirement } from "./x402.ts";

export const TGAS = 1_000_000_000_000n;

/** Default RPC per network. The official rpc.*.near.org endpoints are
 *  deprecated (HTTP 429 since 2026-11); FASTNEAR's free tier is used. */
export const DEFAULT_RPC: Record<string, string> = {
  "near:mainnet": "https://free.rpc.fastnear.com",
  "near:testnet": "https://test.rpc.fastnear.com",
};

export type Mode = "direct" | "wallet";

export type PlannedCall = {
  receiverId: string;
  methodName: string;
  args: Record<string, unknown>;
  gas: bigint;
  deposit: bigint;
};

/**
 * The single function call that pays `requirement`:
 * - direct: `ft_transfer` on the asset, 1 yoctoNEAR attached (needs a
 *   FullAccess key, because function-call keys cannot attach a deposit);
 * - wallet: the policy wallet's `pay`, zero deposit (works with a
 *   function-call key limited to `pay`); the wallet sends the ft_transfer.
 * Never ft_transfer_call: the server rejects it (the token may refund).
 */
export function planPayment(requirement: Requirement, mode: Mode, wallet?: string): PlannedCall {
  const memo = requirement.extra?.memo;
  if (!memo) throw new Error("the requirement carries no extra.memo");
  if (!/^[0-9]+$/.test(requirement.amount)) throw new Error("the requirement amount is not an atomic integer string");
  if (requirement.extra?.assetTransferMethod !== "near-tx-proof") throw new Error("not a near-tx-proof requirement");
  if (mode === "direct") {
    return {
      receiverId: requirement.asset,
      methodName: "ft_transfer",
      args: { receiver_id: requirement.payTo, amount: requirement.amount, memo },
      gas: 30n * TGAS,
      deposit: 1n,
    };
  }
  if (!wallet) throw new Error("wallet mode needs the policy wallet account id");
  return {
    receiverId: wallet,
    methodName: "pay",
    args: { token: requirement.asset, receiver_id: requirement.payTo, amount: requirement.amount, memo },
    gas: 100n * TGAS,
    deposit: 0n,
  };
}

/** Mainnet writes need an exact, operator-approved plan; the CLI refuses
 *  them unless explicitly allowed. */
export function assertNetworkAllowed(network: string, allowMainnet: boolean): void {
  if (network === "near:mainnet" && !allowMainnet) {
    throw new Error("refusing a mainnet payment without --allow-mainnet (mainnet writes need an approved plan)");
  }
  if (!(network in DEFAULT_RPC)) throw new Error(`unsupported network ${network}`);
}
