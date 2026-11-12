import test from "node:test";
import assert from "node:assert/strict";
import { selectKeysToDelete } from "../lib/keys.js";

const WALLET = "wallet.testnet";
const OLD = "old-wallet.testnet";

function pay(publicKey, receiver, allowance = "10") {
  return {
    public_key: publicKey,
    access_key: {
      permission: {
        FunctionCall: {
          allowance,
          method_names: ["pay"],
          receiver_id: receiver,
        },
      },
    },
  };
}

function full(publicKey) {
  return { public_key: publicKey, access_key: { permission: "FullAccess" } };
}

test("cleanup drops the leftover pay key and keeps the current wallet key", () => {
  const plan = selectKeysToDelete(
    [full("ed25519:full"), pay("ed25519:current", WALLET, "9"), pay("ed25519:old", OLD, "8")],
    { walletId: WALLET, keepPublicKey: "ed25519:current", dropFullAccess: false },
  );
  assert.equal(plan.keep, "ed25519:current");
  assert.deepEqual(plan.drop, ["ed25519:old"]);
});

test("cleanup can drop the full-access key only when a pay key remains", () => {
  const keys = [full("ed25519:full"), pay("ed25519:current", WALLET)];
  const kept = selectKeysToDelete(keys, { walletId: WALLET, dropFullAccess: false });
  assert.deepEqual(kept.drop, []);
  const dropped = selectKeysToDelete(keys, { walletId: WALLET, dropFullAccess: true });
  assert.deepEqual(dropped.drop, ["ed25519:full"]);
  const onlyFull = selectKeysToDelete([full("ed25519:full")], { walletId: WALLET, dropFullAccess: true });
  assert.equal(onlyFull.keep, null);
  assert.deepEqual(onlyFull.drop, []);
});

test("when several pay keys match, the named public key wins", () => {
  const plan = selectKeysToDelete(
    [pay("ed25519:a", WALLET, "1"), pay("ed25519:b", WALLET, "99")],
    { walletId: WALLET, keepPublicKey: "ed25519:a", dropFullAccess: false },
  );
  assert.equal(plan.keep, "ed25519:a");
  assert.deepEqual(plan.drop, ["ed25519:b"]);
});
