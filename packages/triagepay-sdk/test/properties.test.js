import test from "node:test";
import assert from "node:assert/strict";
import fc from "fast-check";
import {
  buildPaymentPayload,
  clipMemo,
  decodeHeader,
  encodeHeader,
  policyPayCall,
  selectRequirement,
} from "../src/index.js";

const WALLET = "wallet.testnet";
const TOKEN = "token.testnet";
const PAYEE = "payee.testnet";

test("atomic amounts round-trip and a non-integer is refused", () => {
  fc.assert(
    fc.property(fc.bigInt({ min: 1n, max: 10n ** 40n }), (units) => {
      const call = policyPayCall({
        wallet: WALLET,
        token: TOKEN,
        receiverId: PAYEE,
        amount: units.toString(),
      });
      assert.equal(call.methodName, "pay");
      assert.equal(call.deposit, "0");
      assert.equal(call.receiverId, WALLET);
      assert.equal(call.args.amount, units.toString());
      assert.equal(call.args.token, TOKEN);
      assert.equal(call.args.receiver_id, PAYEE);
    }),
  );
  for (const amount of ["0", "-1", "1.5", "", "1e2", "nope"]) {
    assert.throws(() => policyPayCall({
      wallet: WALLET,
      token: TOKEN,
      receiverId: PAYEE,
      amount,
    }));
  }
  fc.assert(
    fc.property(fc.string({ maxLength: 80 }), (amount) => {
      let call;
      try {
        call = policyPayCall({ wallet: WALLET, token: TOKEN, receiverId: PAYEE, amount });
      } catch {
        return;
      }
      const units = BigInt(String(amount).trim());
      assert.ok(units > 0n);
      assert.equal(call.args.amount, units.toString());
      assert.equal(call.deposit, "0");
      assert.equal(call.methodName, "pay");
    }),
  );
});

test("a memo is clipped to 256 utf-8 bytes and stays a prefix", () => {
  fc.assert(
    fc.property(fc.string({ maxLength: 400 }), (memo) => {
      const clipped = clipMemo(memo);
      const bytes = Buffer.from(clipped, "utf8");
      assert.ok(bytes.length <= 256);
      assert.ok(memo.startsWith(clipped));
      const call = policyPayCall({
        wallet: WALLET,
        token: TOKEN,
        receiverId: PAYEE,
        amount: "1",
        memo,
      });
      assert.equal(call.args.memo, clipped);
    }),
  );
  assert.equal(clipMemo(null), null);
  assert.equal(clipMemo("é".repeat(200)).length <= 256, true);
  assert.ok(Buffer.from(clipMemo("é".repeat(200)), "utf8").length <= 256);
});

test("x402 headers round-trip and selection stays inside the near-tx-proof shape", () => {
  fc.assert(
    fc.property(
      fc.jsonValue().filter((value) => value !== undefined),
      (value) => {
        assert.deepEqual(decodeHeader(encodeHeader(value)), value);
      },
    ),
  );
  const requirement = () => fc.record({
    scheme: fc.constantFrom("exact", "other"),
    network: fc.constantFrom("near:mainnet", "near:testnet", "eip155:84532"),
    amount: fc.bigInt({ min: 1n, max: 1_000_000n }).map((n) => n.toString()),
    asset: fc.constant(TOKEN),
    payTo: fc.constant(PAYEE),
    extra: fc.record({
      assetTransferMethod: fc.constantFrom("near-tx-proof", "evm"),
      paymentFlow: fc.constantFrom("upfront", "later"),
      memo: fc.option(fc.string({ minLength: 1, maxLength: 40 }), { nil: undefined }),
    }),
  });
  fc.assert(
    fc.property(fc.array(requirement(), { maxLength: 6 }), fc.constantFrom("near:mainnet", undefined), (accepts, network) => {
      const body = { x402Version: 2, accepts };
      let selected = null;
      try {
        selected = selectRequirement(body, network);
      } catch {
        selected = null;
      }
      if (selected == null) return;
      assert.equal(selected.scheme, "exact");
      assert.equal(selected.extra.assetTransferMethod, "near-tx-proof");
      assert.equal(selected.extra.paymentFlow || "upfront", "upfront");
      assert.ok(selected.extra.memo);
      if (network !== undefined) assert.equal(selected.network, network);
      const payload = buildPaymentPayload(selected, { tx_hash: "abc", signer_id: "agent.testnet" });
      const decoded = decodeHeader(encodeHeader(payload));
      assert.equal(decoded.x402Version, 2);
      assert.equal(decoded.payload.tx_hash, "abc");
      assert.equal(decoded.payload.intent_id, selected.extra.memo);
      assert.equal(decoded.payload.signer_id, "agent.testnet");
      assert.deepEqual(decoded.accepted, JSON.parse(JSON.stringify(selected)));
    }),
  );
});
