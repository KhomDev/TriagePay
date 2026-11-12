// EIP-1559 helpers for a Chain Signatures request. No dependency.
//
// eip1559SigningHash() gives the 32 bytes v1.signer must sign.
// evmAddressFromNearKey() turns the signer's `derived_public_key`
// answer ("secp256k1:<base58 of 64 bytes>") into the EVM address.
// signedEip1559() attaches the MPC signature for broadcast.

import { keccak256 } from "./keccak.js";

const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

export function base58Decode(text) {
  let n = 0n;
  for (const ch of String(text)) {
    const i = B58.indexOf(ch);
    if (i < 0) throw new Error("not base58");
    n = n * 58n + BigInt(i);
  }
  let hex = n.toString(16);
  if (hex.length % 2) hex = "0" + hex;
  const body = n === 0n ? Buffer.alloc(0) : Buffer.from(hex, "hex");
  let zeros = 0;
  for (const ch of String(text)) {
    if (ch !== "1") break;
    zeros++;
  }
  return Buffer.concat([Buffer.alloc(zeros), body]);
}

function toBytes(value) {
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) return Buffer.from(value);
  if (typeof value === "string") {
    const hex = value.replace(/^0x/, "");
    if (hex.length % 2) throw new Error("odd hex length");
    return Buffer.from(hex, "hex");
  }
  if (typeof value === "bigint" || typeof value === "number") {
    const n = BigInt(value);
    if (n < 0n) throw new Error("negative integer");
    if (n === 0n) return Buffer.alloc(0);
    let hex = n.toString(16);
    if (hex.length % 2) hex = "0" + hex;
    return Buffer.from(hex, "hex");
  }
  throw new Error("cannot RLP-encode " + typeof value);
}

function lengthPrefix(len, offset) {
  if (len < 56) return Buffer.from([offset + len]);
  const bytes = toBytes(BigInt(len));
  return Buffer.concat([Buffer.from([offset + 55 + bytes.length]), bytes]);
}

export function rlpEncode(item) {
  if (Array.isArray(item)) {
    const body = Buffer.concat(item.map(rlpEncode));
    return Buffer.concat([lengthPrefix(body.length, 0xc0), body]);
  }
  const bytes = toBytes(item);
  if (bytes.length === 1 && bytes[0] < 0x80) return bytes;
  return Buffer.concat([lengthPrefix(bytes.length, 0x80), bytes]);
}

function fields(tx) {
  return [
    BigInt(tx.chainId),
    BigInt(tx.nonce),
    BigInt(tx.maxPriorityFeePerGas),
    BigInt(tx.maxFeePerGas),
    BigInt(tx.gasLimit),
    tx.to,
    BigInt(tx.value || 0),
    tx.data || "0x",
    [],
  ];
}

/** keccak256(0x02 || rlp(unsigned fields)), as hex without 0x. */
export function eip1559SigningHash(tx) {
  const encoded = Buffer.concat([Buffer.from([2]), rlpEncode(fields(tx))]);
  return keccak256(encoded).toString("hex");
}

/** Raw signed transaction (hex with 0x) and its hash. */
export function signedEip1559(tx, { r, s, yParity }) {
  const strip = (hex) => BigInt("0x" + String(hex).replace(/^0x/, ""));
  const raw = Buffer.concat([
    Buffer.from([2]),
    rlpEncode([...fields(tx), BigInt(yParity), strip(r), strip(s)]),
  ]);
  return { raw: "0x" + raw.toString("hex"), hash: "0x" + keccak256(raw).toString("hex") };
}

/** EVM address for a NEAR "secp256k1:<base58>" public key (64 or 65 bytes). */
export function evmAddressFromNearKey(nearKey) {
  const text = String(nearKey).replace(/^secp256k1:/, "");
  let bytes = base58Decode(text);
  if (bytes.length === 65 && bytes[0] === 4) bytes = bytes.subarray(1);
  if (bytes.length !== 64) throw new Error("expected a 64-byte uncompressed secp256k1 key");
  return "0x" + keccak256(bytes).subarray(12).toString("hex");
}

/**
 * Normalize a v1.signer ECDSA answer to { r, s, yParity }.
 * Accepts { big_r: { affine_point }, s: { scalar }, recovery_id } with or
 * without a { Secp256k1: ... } wrapper.
 */
export function normalizeMpcSignature(answer) {
  const sig = answer && answer.Secp256k1 ? answer.Secp256k1 : answer;
  const point = String(sig.big_r.affine_point).replace(/^0x/, "");
  const r = point.length === 66 ? point.slice(2) : point;
  const s = String(sig.s.scalar).replace(/^0x/, "");
  return { r, s, yParity: Number(sig.recovery_id) };
}
