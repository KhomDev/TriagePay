import test from "node:test";
import assert from "node:assert/strict";
import {
  eip1559SigningHash,
  evmAddressFromNearKey,
  normalizeMpcSignature,
  rlpEncode,
  signedEip1559,
} from "../src/index.js";

// Vectors from the testnet request recorded in
// docs/demo/testnet-chainsig/2026-11-12/EVIDENCE.json. ethers v6 gives the
// same unsigned hash, sender, raw transaction and hash for this input.
const TX = {
  chainId: 84532,
  nonce: "0",
  maxPriorityFeePerGas: "1000000",
  maxFeePerGas: "13000000",
  gasLimit: "80000",
  to: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
  value: "0",
  data: "0xa9059cbb000000000000000000000000000000000000000000000000000000000000dead0000000000000000000000000000000000000000000000000000000000002710",
};

test("rlp matches the spec examples", () => {
  assert.equal(rlpEncode("0x646f67").toString("hex"), "83646f67");
  assert.equal(rlpEncode([]).toString("hex"), "c0");
  assert.equal(rlpEncode(0n).toString("hex"), "80");
  assert.equal(rlpEncode(15n).toString("hex"), "0f");
  assert.equal(rlpEncode(1024n).toString("hex"), "820400");
  assert.equal(rlpEncode(["0x636174", "0x646f67"]).toString("hex"), "c88363617483646f67");
});

test("the derived NEAR key maps to the recorded Base Sepolia address", () => {
  assert.equal(
    evmAddressFromNearKey("secp256k1:4BktUHkTqEmDUtWneJsiC4LFjFGuWp5P1eMQ3VeUCMtMdsMvKnjtPueNVpeNYR8aRzpeSgTTf4KJuBQTNMzVimZV"),
    "0x508e984fda3bc37d8c33bfb5098441fd2169fcc8",
  );
});

test("the EIP-1559 hash and signed transaction match the recorded request", () => {
  assert.equal(eip1559SigningHash(TX), "f228b2c29745e87c425566d3c4bc2cf11d522c7ec3cd23a5bb3a912d39ef16a1");
  const sig = normalizeMpcSignature({
    big_r: { affine_point: "0211c3aa365f431bd51d6676bab903192d18e347aec626aa3c022d3687fe0af2e7" },
    s: { scalar: "011d446a5756ccdcd73f93ad277c2214eab6aab9d767844fafe161a8ecd7c361" },
    recovery_id: 1,
  });
  const signed = signedEip1559(TX, sig);
  assert.equal(signed.hash, "0x21744b81fb83b61891185ab869d3a1c4bc8e0aaa8d690467ef7cf3f0b0c8f188");
});

test("red-team T5-4: RLP refuses invalid hex instead of truncating it", () => {
  assert.throws(() => rlpEncode("0x12zz"), /invalid hex/);
  assert.throws(() => rlpEncode("0xgg"), /invalid hex/);
  assert.equal(rlpEncode("0x").length, 1);
});
