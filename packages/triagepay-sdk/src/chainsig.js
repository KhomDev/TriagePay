// Partial Chain Signatures proof of concept.
//
// This builds the bytes a policy wallet would ask NEAR to sign for an
// ERC-20 transfer, and the `sign` arguments for v1.signer. It does not
// derive an EVM address, request a signature, or broadcast a transaction.
// See docs/contracts/chain-signatures.md.

import { keccak256Hex } from "./keccak.js";

export const TESTNET_SIGNER = "v1.signer-prod.testnet";
export const MAINNET_SIGNER = "v1.signer";
/** Circle's published USDC on Base Sepolia. Not called from this package. */
export const BASE_SEPOLIA_USDC = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";

export function erc20TransferCalldata(to, amount) {
  const addr = String(to || "").toLowerCase().replace(/^0x/, "");
  if (!/^[0-9a-f]{40}$/.test(addr)) throw new Error("recipient must be a 20-byte hex address");
  const units = BigInt(amount);
  if (units < 0n || units >= 1n << 256n) throw new Error("amount is outside uint256");
  const selector = keccak256Hex(Buffer.from("transfer(address,uint256)")).slice(0, 8);
  return "0x" + selector + addr.padStart(64, "0") + units.toString(16).padStart(64, "0");
}

/**
 * Arguments for `sign` on the NEAR chain-signature contract.
 * `payloadHex` must already be the 32-byte keccak256 of the unsigned
 * EIP-1559 transaction. This function does not RLP-encode that transaction.
 */
export function ecdsaSignRequest({ path = "base-sepolia-1", payloadHex, domainId = 0, testnet = true }) {
  const hex = String(payloadHex || "").toLowerCase().replace(/^0x/, "");
  if (!/^[0-9a-f]{64}$/.test(hex)) throw new Error("ECDSA payload must be 32 bytes of hex");
  return {
    signer: testnet ? TESTNET_SIGNER : MAINNET_SIGNER,
    method: "sign",
    depositYocto: "1",
    gasTgas: 300,
    args: {
      request: {
        path,
        payload_v2: { Ecdsa: hex },
        domain_id: domainId,
      },
    },
    note: "Request only. Nothing is submitted by this function.",
  };
}
