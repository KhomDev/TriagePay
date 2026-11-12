import test from "node:test";
import assert from "node:assert/strict";
import {
  BASE_SEPOLIA_USDC,
  buildPaymentPayload,
  createPolicyPayer,
  decodeHeader,
  ecdsaSignRequest,
  encodeHeader,
  erc20TransferCalldata,
  keccak256Hex,
  nearAiPaymentTool,
  payPerCall,
  policyPayCall,
  selectRequirement,
} from "../src/index.js";

test("keccak256 matches the empty and abc vectors", () => {
  assert.equal(keccak256Hex(Buffer.alloc(0)), "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");
  assert.equal(keccak256Hex(Buffer.from("abc")), "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45");
});

test("policyPayCall is a zero-deposit pay on the wallet", () => {
  const call = policyPayCall({
    wallet: "wallet.testnet",
    token: "wrap.testnet",
    receiverId: "payee.testnet",
    amount: "1000",
    memo: "job-1",
  });
  assert.equal(call.receiverId, "wallet.testnet");
  assert.equal(call.methodName, "pay");
  assert.equal(call.deposit, "0");
  assert.deepEqual(call.args, {
    token: "wrap.testnet",
    receiver_id: "payee.testnet",
    amount: "1000",
    memo: "job-1",
  });
  assert.throws(() => policyPayCall({ wallet: "w", token: "t", receiverId: "p", amount: "0" }));
});

test("the payer returns an x402 proof from an injected signer", async () => {
  const payer = createPolicyPayer({
    wallet: "wallet.testnet",
    signerId: "agent.testnet",
    sign: async (call) => {
      assert.equal(call.args.receiver_id, "gateway.testnet");
      assert.equal(call.args.memo, "memo-9");
      return "txhash";
    },
  });
  const proof = await payer({
    asset: "token.testnet",
    payTo: "gateway.testnet",
    amount: "5",
    extra: { memo: "memo-9" },
  });
  assert.deepEqual(proof, { tx_hash: "txhash", signer_id: "agent.testnet" });
});

test("payPerCall retries a 402 with the payment header", async () => {
  const requirement = {
    scheme: "exact",
    network: "near:testnet",
    amount: "5",
    asset: "token.testnet",
    payTo: "gateway.testnet",
    extra: { assetTransferMethod: "near-tx-proof", paymentFlow: "upfront", memo: "memo-9" },
  };
  const calls = [];
  const fetchImpl = async (_url, init) => {
    calls.push(init.headers["payment-signature"] || null);
    if (calls.length === 1) {
      return {
        status: 402,
        headers: { get: (name) => (name.toLowerCase() === "payment-required" ? encodeHeader({ x402Version: 2, accepts: [requirement] }) : null) },
      };
    }
    return { status: 200, headers: { get: () => null }, body: { verdict: "supported" } };
  };
  const result = await payPerCall({
    fetchImpl,
    url: "http://api.local/v2/scan",
    body: { finding: 1 },
    payer: async () => ({ tx_hash: "abc", signer_id: "agent.testnet" }),
  });
  assert.equal(result.paid, true);
  assert.equal(result.response.status, 200);
  const payload = decodeHeader(calls[1]);
  assert.equal(payload.payload.tx_hash, "abc");
  assert.equal(payload.payload.intent_id, "memo-9");
  assert.equal(selectRequirement({ x402Version: 2, accepts: [requirement] }).payTo, "gateway.testnet");
  assert.equal(buildPaymentPayload(requirement, { tx_hash: "abc", signer_id: "agent.testnet" }).payload.signer_id, "agent.testnet");
});

test("the NEAR AI tool can only build a wallet pay", async () => {
  const tool = nearAiPaymentTool({
    wallet: "wallet.testnet",
    token: "wrap.testnet",
    sign: async (call) => {
      assert.equal(call.methodName, "pay");
      assert.equal(call.deposit, "0");
      return "signed";
    },
  });
  assert.equal(tool.function.name, "triagepay_pay");
  const result = await tool.execute({ receiver_id: "payee.testnet", amount: "3", memo: "m" });
  assert.equal(result.tx_hash, "signed");
  assert.equal(result.call.args.token, "wrap.testnet");
});

test("erc20 calldata uses the transfer selector and the sign request is not sent", () => {
  const data = erc20TransferCalldata("0x" + "11".repeat(20), 1n);
  assert.match(data, /^0xa9059cbb/);
  assert.equal(data.length, 2 + 8 + 64 + 64);
  const request = ecdsaSignRequest({
    payloadHex: "ab".repeat(32),
  });
  assert.equal(request.signer, "v1.signer-prod.testnet");
  assert.equal(request.depositYocto, "1");
  assert.equal(request.args.request.payload_v2.Ecdsa, "ab".repeat(32));
  assert.equal(request.args.request.path, "base-sepolia-1");
  assert.match(request.note, /Nothing is submitted/);
  assert.match(BASE_SEPOLIA_USDC, /^0x[0-9a-fA-F]{40}$/);
  assert.throws(() => ecdsaSignRequest({ payloadHex: "abcd" }));
});
