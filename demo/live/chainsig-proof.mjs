// One real Chain Signatures request on NEAR testnet.
//
//   node chainsig-proof.mjs
//
// The owner account of the testnet demo wallet asks v1.signer-prod.testnet
// to sign an EIP-1559 USDC transfer on Base Sepolia (chain 84532). The
// script checks that the signature recovers to the address the signer
// derives for (owner, path), and broadcasts only when that address holds
// Base Sepolia ETH for gas. Keys come from demo/live/.keys and are not
// printed or written.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { secp256k1 } from "@noble/curves/secp256k1.js";
import { sha3_256 } from "@noble/hashes/sha3.js";
import * as nearApi from "near-api-js";
import { viewCall } from "./lib/near.js";
import {
  BASE_SEPOLIA_USDC,
  TESTNET_SIGNER,
  base58Decode,
  ecdsaSignRequest,
  eip1559SigningHash,
  erc20TransferCalldata,
  keccak256,
  evmAddressFromNearKey,
  normalizeMpcSignature,
  signedEip1559,
} from "../../packages/triagepay-sdk/src/index.js";

const ROOT = dirname(fileURLToPath(import.meta.url));
const REPO = join(ROOT, "..", "..");
const OUT = join(REPO, "docs", "demo", "testnet-chainsig", "2026-11-12");
const config = JSON.parse(readFileSync(join(ROOT, "config.json"), "utf8"));
const RPC = config.rpcUrl;
const BASE_RPC = process.env.BASE_SEPOLIA_RPC || "https://sepolia.base.org";
const PATH = "base-sepolia-1";
const RECIPIENT = "0x000000000000000000000000000000000000dEaD";
const AMOUNT = 10000n; // 0.01 USDC (6 decimals)

async function evm(method, params) {
  const res = await fetch(BASE_RPC, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
    signal: AbortSignal.timeout(20000),
  });
  const body = await res.json();
  if (body.error) throw new Error(`${method}: ${JSON.stringify(body.error).slice(0, 200)}`);
  return body.result;
}

const hex = (bytes) => Buffer.from(bytes).toString("hex");
const pointFromNear = (key) => secp256k1.Point.fromHex("04" + hex(base58Decode(String(key).replace(/^secp256k1:/, ""))));

async function main() {
  const owner = JSON.parse(readFileSync(join(ROOT, ".keys", "owner.json"), "utf8"));
  const predecessor = owner.account_id;
  const root = await viewCall(RPC, TESTNET_SIGNER, "public_key", { domain_id: 0 });
  const derived = await viewCall(RPC, TESTNET_SIGNER, "derived_public_key", { path: PATH, predecessor, domain_id: 0 });

  // Cross-check the signer's answer with the published derivation:
  // child = root + sha3_256("near-mpc-recovery v0.1.0 epsilon derivation:" + predecessor + "," + path) * G
  const eps = BigInt("0x" + hex(sha3_256(Buffer.from(`near-mpc-recovery v0.1.0 epsilon derivation:${predecessor},${PATH}`))));
  const local = pointFromNear(root).add(secp256k1.Point.BASE.multiply(eps % secp256k1.Point.CURVE().n));
  const derivedPoint = pointFromNear(derived);
  const derivationMatches = local.equals(derivedPoint);
  const address = evmAddressFromNearKey(derived);
  console.log(`derived ${address} (local derivation ${derivationMatches ? "matches" : "DIFFERS"})`);

  const nonce = BigInt(await evm("eth_getTransactionCount", [address, "pending"]));
  const balance = BigInt(await evm("eth_getBalance", [address, "latest"]));
  const gasPrice = BigInt(await evm("eth_gasPrice", []));
  const tip = 1_000_000n;
  const tx = {
    chainId: 84532,
    nonce: nonce.toString(),
    maxPriorityFeePerGas: tip.toString(),
    maxFeePerGas: (gasPrice * 2n + tip).toString(),
    gasLimit: "80000",
    to: BASE_SEPOLIA_USDC,
    value: "0",
    data: erc20TransferCalldata(RECIPIENT, AMOUNT),
  };
  const payload = eip1559SigningHash(tx);
  const request = ecdsaSignRequest({ path: PATH, payloadHex: payload });
  console.log(`payload ${payload}`);

  const provider = new nearApi.JsonRpcProvider({ url: RPC });
  const account = new nearApi.Account(predecessor, provider, owner.secret_key);
  const result = await account.signAndSendTransaction({
    receiverId: request.signer,
    actions: [nearApi.actions.functionCall("sign", request.args, BigInt(request.gasTgas) * 10n ** 12n, BigInt(request.depositYocto))],
    waitUntil: "FINAL",
    throwOnFailure: false,
  });
  const nearTx = result.transaction.hash;
  console.log(`sign tx https://testnet.nearblocks.io/txns/${nearTx}`);
  if (!result.status || result.status.SuccessValue == null) {
    throw new Error("sign failed: " + JSON.stringify(result.status).slice(0, 300));
  }
  const answer = JSON.parse(Buffer.from(result.status.SuccessValue, "base64").toString("utf8"));
  const sig = normalizeMpcSignature(answer);
  const signature = new secp256k1.Signature(BigInt("0x" + sig.r), BigInt("0x" + sig.s), sig.yParity);
  const recovered = signature.recoverPublicKey(Buffer.from(payload, "hex"));
  const recoveredAddress = "0x" + keccak256(Buffer.from(recovered.toBytes(false)).subarray(1)).subarray(12).toString("hex");
  const verifies = secp256k1.verify(signature.toBytes("compact"), Buffer.from(payload, "hex"), derivedPoint.toBytes(true), { prehash: false });
  const signed = signedEip1559(tx, sig);
  console.log(`recovered ${recoveredAddress} ${recoveredAddress === address ? "== derived" : "!= derived"}; verify ${verifies}`);

  let broadcast = null;
  if (balance > 0n) {
    try {
      broadcast = { hash: await evm("eth_sendRawTransaction", [signed.raw]) };
    } catch (error) {
      broadcast = { error: String(error.message || error).slice(0, 200) };
    }
  }
  const evidence = {
    date: new Date().toISOString(),
    signer: TESTNET_SIGNER,
    predecessor,
    path: PATH,
    domainId: 0,
    rootPublicKey: root,
    derivedPublicKey: derived,
    derivationMatchesLocal: derivationMatches,
    evmAddress: address,
    baseSepolia: { rpc: BASE_RPC, chainId: 84532, balanceWei: balance.toString(), nonce: nonce.toString() },
    unsignedTx: tx,
    erc20: { token: BASE_SEPOLIA_USDC, recipient: RECIPIENT, amount: AMOUNT.toString(), decimals: 6 },
    payload,
    nearSignTx: nearTx,
    nearSignTxUrl: `https://testnet.nearblocks.io/txns/${nearTx}`,
    mpcAnswer: answer,
    signature: sig,
    recoveredAddress,
    recoversToDerived: recoveredAddress === address,
    verifiesAgainstDerivedKey: verifies,
    signedRawTx: signed.raw,
    evmTxHash: signed.hash,
    broadcast: broadcast || { skipped: "the derived address holds no Base Sepolia ETH for gas" },
  };
  mkdirSync(OUT, { recursive: true });
  writeFileSync(join(OUT, "EVIDENCE.json"), JSON.stringify(evidence, null, 2) + "\n");
  console.log(JSON.stringify({ ...evidence, mpcAnswer: undefined, unsignedTx: undefined, signedRawTx: undefined }, null, 2));
}

main().catch((error) => {
  console.error("STOP: " + String(error && error.message ? error.message : error).replace(/ed25519:[1-9A-HJ-NP-Za-km-z]{60,}/g, "ed25519:<redacted>"));
  process.exit(1);
});
