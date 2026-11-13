import test from "node:test";
import assert from "node:assert/strict";
import {
  USDC_CONTRACT,
  nearDepositCall,
  quoteRequest,
  requestQuote,
  toAtomic,
  usdcToken,
} from "../public/intents.js";

const USDC = {
  assetId: "nep141:" + USDC_CONTRACT,
  blockchain: "near",
  symbol: "USDC",
  decimals: 6,
  contractAddress: USDC_CONTRACT,
};
const WNEAR = {
  assetId: "nep141:wrap.near",
  blockchain: "near",
  symbol: "wNEAR",
  decimals: 24,
  contractAddress: "wrap.near",
};

test("USDC is found by the Circle contract, not by a guessed asset id", () => {
  assert.equal(usdcToken([WNEAR, USDC]).assetId, "nep141:" + USDC_CONTRACT);
  assert.equal(usdcToken([WNEAR]), null);
});

test("amounts become atomic units without floating point", () => {
  assert.equal(toAtomic("0.1", 6), "100000");
  assert.equal(toAtomic("1", 24), "1" + "0".repeat(24));
  assert.throws(() => toAtomic("0.0001", 3), /decimal places/);
  assert.throws(() => toAtomic("0", 6), /positive/);
});

test("a quote body uses the documented fields and a destination from the token list", () => {
  const body = quoteRequest({
    originAsset: WNEAR.assetId,
    destinationAsset: USDC.assetId,
    amount: "1000",
    recipient: "wallet.near",
    refundTo: "owner.near",
    dry: true,
    deadline: "2026-11-13T06:00:00.000Z",
  });
  assert.equal(body.dry, true);
  assert.equal(body.swapType, "EXACT_INPUT");
  assert.equal(body.depositType, "ORIGIN_CHAIN");
  assert.equal(body.recipientType, "DESTINATION_CHAIN");
  assert.equal(body.refundType, "ORIGIN_CHAIN");
  assert.equal(body.destinationAsset, USDC.assetId);
  assert.equal(body.deadline, "2026-11-13T06:00:00.000Z");
  assert.throws(() => quoteRequest({ originAsset: WNEAR.assetId }), /required/);
});

test("a NEAR deposit is an ft_transfer of 1 yocto to the quote address", () => {
  const call = nearDepositCall({
    contract: "wrap.near",
    depositAddress: "abcdef.near",
    amount: "1000",
    memo: "note",
  });
  assert.equal(call.methodName, "ft_transfer");
  assert.equal(call.deposit, "1");
  assert.equal(call.args.receiver_id, "abcdef.near");
  assert.equal(call.args.memo, "note");
  const plain = nearDepositCall({ contract: "wrap.near", depositAddress: "abcdef.near", amount: "1000" });
  assert.equal(Object.hasOwn(plain.args, "memo"), false);
});

test("requestQuote posts to /v0/quote and surfaces the API message", async () => {
  const calls = [];
  const fetchImpl = async (url, init) => {
    calls.push({ url, init });
    return { ok: false, status: 400, text: async () => JSON.stringify({ message: "bad amount" }) };
  };
  await assert.rejects(
    requestQuote({ dry: true }, fetchImpl, "https://1click.example"),
    /400: bad amount/,
  );
  assert.equal(calls[0].url, "https://1click.example/v0/quote");
  assert.equal(calls[0].init.method, "POST");
});
