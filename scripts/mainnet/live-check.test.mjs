import test from "node:test";
import assert from "node:assert/strict";
import { checkHealth, checkKeys, checkProof, checkWallet } from "./live-check.mjs";

const wallet = {
  accountId: "wallet.near",
  codeHash: "hash",
  config: { owner: "owner.near", paused: false, tokens: ["wrap.near"] },
  agent: {
    accountId: "agent.near",
    perCallMax: "10",
    dailyMax: "40",
    payees: ["gateway.near"],
    active: true,
    keys: "pay-only",
    publicKey: "ed25519:agent",
  },
  review: { approvalThreshold: "5", reviewUnknownPayees: true, maxPending: 8 },
  walletKeys: "none",
};

function observed(overrides = {}) {
  return {
    exists: true,
    codeHash: "hash",
    config: { owner: "owner.near", paused: false, tokens: ["wrap.near"] },
    agent: {
      per_call_max: "10",
      daily_max: "40",
      payees: ["gateway.near"],
      active: true,
    },
    review: { approval_threshold: "5", review_unknown_payees: true, max_pending: 8 },
    keys: [{
      public_key: "ed25519:agent",
      permission: {
        FunctionCall: {
          allowance: "1",
          receiver_id: "wallet.near",
          method_names: ["pay"],
        },
      },
    }],
    walletKeys: [],
    ...overrides,
  };
}

const payKey = {
  permission: {
    FunctionCall: { receiver_id: "wallet.near", method_names: ["pay"], allowance: "99" },
  },
};

test("a pay-only key matches and a full-access key does not", () => {
  assert.deepEqual(checkKeys("pay-only", "wallet.near", [payKey]), []);
  assert.deepEqual(checkKeys("none", "wallet.near", []), []);
  assert.ok(checkKeys("none", "wallet.near", [payKey]).length > 0);
  assert.ok(checkKeys("pay-only", "wallet.near", []).length > 0);
  assert.ok(checkKeys("pay-only", "wallet.near", [{ permission: "FullAccess" }]).length > 0);
  assert.ok(checkKeys("pay-only", "wallet.near", [{
    permission: { FunctionCall: { receiver_id: "other.near", method_names: ["pay"] } },
  }]).length > 0);
  assert.ok(checkKeys("pay-only", "wallet.near", [{
    permission: { FunctionCall: { receiver_id: "wallet.near", method_names: ["pay", "withdraw"] } },
  }]).length > 0);
});

test("wallet views must match the recorded owner, token, caps, and code hash", () => {
  assert.deepEqual(checkWallet(wallet, observed()), []);
  const drifted = observed();
  drifted.agent = { ...drifted.agent, daily_max: "1" };
  assert.ok(checkWallet(wallet, drifted).some((line) => line.includes("daily cap")));
  assert.ok(checkWallet(wallet, { exists: false }).some((line) => line.includes("does not exist")));
  const revoked = {
    ...wallet,
    agent: { ...wallet.agent, active: false, keys: "none", publicKey: undefined },
  };
  const quiet = observed({ agent: { ...observed().agent, active: false }, keys: [] });
  assert.deepEqual(checkWallet(revoked, quiet), []);
  const rich = observed();
  rich.agent = { ...rich.agent, spent_in_window: "999" };
  rich.keys = [{ ...rich.keys[0], permission: { FunctionCall: { ...rich.keys[0].permission.FunctionCall, allowance: "1" } } }];
  assert.deepEqual(checkWallet(wallet, rich), []);
});

test("proof transactions keep their outcome and health must report ok", () => {
  const paid = {
    label: "paid",
    outcome: "success",
    events: ["pay_requested", "pay_settled"],
  };
  assert.deepEqual(checkProof(paid, {
    finalExecutionStatus: "FINAL",
    success: true,
    events: ["pay_requested", "pay_settled"],
  }), []);
  assert.ok(checkProof(paid, {
    finalExecutionStatus: "FINAL",
    success: true,
    events: ["pay_requested"],
  }).some((line) => line.includes("pay_settled")));
  const refused = { label: "refused", outcome: "failure", errorCode: "ERR_PAYEE_NOT_ALLOWED" };
  assert.deepEqual(checkProof(refused, {
    finalExecutionStatus: "FINAL",
    success: false,
    failureText: "Smart contract panicked: ERR_PAYEE_NOT_ALLOWED: receiver is not on the payee allowlist",
  }), []);
  assert.ok(checkProof(refused, {
    finalExecutionStatus: "FINAL",
    success: false,
    failureText: "ERR_AGENT_INACTIVE",
  }).length > 0);
  assert.deepEqual(checkHealth({ ok: true, wallet: "wallet.testnet" }, {
    status: 200,
    body: { ok: true, wallet: "wallet.testnet" },
  }), []);
  assert.ok(checkHealth({ ok: true }, { status: 500, body: { ok: false } }).length > 0);
});
