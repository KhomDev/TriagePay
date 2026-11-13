import test from "node:test";
import assert from "node:assert/strict";
import { diffPolicy, proposePolicy, validatePolicy } from "../lib/policy-nl.js";

const context = { decimals: 6, triagePayee: "api.testnet" };

test("validatePolicy rejects a value outside the schema", () => {
  assert.equal(validatePolicy(null).ok, false);
  assert.equal(validatePolicy([]).ok, false);
  assert.equal(validatePolicy({ extra: 1 }).ok, false);
  assert.equal(validatePolicy({ dailyMaxAtomic: 1 }).ok, false);
  assert.equal(validatePolicy({ dailyMaxAtomic: "12.5" }).ok, false);
  assert.equal(validatePolicy({ payees: "api.testnet" }).ok, false);
  assert.equal(validatePolicy({ payees: Array.from({ length: 9 }, () => "a.testnet") }).ok, false);
  assert.equal(validatePolicy({ payees: ["not a near account"] }).ok, false);
  assert.equal(validatePolicy({ reviewUnknownPayees: "yes" }).ok, false);
  const ok = validatePolicy({
    dailyMaxAtomic: "10",
    payees: ["API.Testnet"],
    reviewUnknownPayees: false,
  });
  assert.equal(ok.ok, true);
  assert.deepEqual(ok.proposal.payees, ["api.testnet"]);
  assert.equal(ok.proposal.perCallMaxAtomic, null);
});

test("diffPolicy names only the fields that change", () => {
  const current = {
    dailyMaxAtomic: "10",
    perCallMaxAtomic: "5",
    payees: ["old.testnet"],
    approvalThresholdAtomic: "1",
    reviewUnknownPayees: false,
  };
  assert.deepEqual(diffPolicy(current, {
    dailyMaxAtomic: "10",
    perCallMaxAtomic: null,
    payees: null,
    approvalThresholdAtomic: null,
    reviewUnknownPayees: null,
  }, 6, "USDC"), []);
  const changes = diffPolicy(current, {
    dailyMaxAtomic: "20",
    perCallMaxAtomic: "6",
    payees: ["new.testnet"],
    approvalThresholdAtomic: "2",
    reviewUnknownPayees: true,
  }, 6, "USDC");
  assert.deepEqual(changes.map((row) => row.field), [
    "daily max",
    "per-call max",
    "approval threshold",
    "allowlist",
    "unknown payees",
  ]);
});

function response({ ok = true, status = 200, json, text }) {
  return {
    ok,
    status,
    json: async () => json,
    text: async () => text,
    body: { cancel: async () => {} },
  };
}

test("proposePolicy keeps the parser when the model is missing or unusable", async () => {
  const bare = await proposePolicy({ text: "max 1 near a day", context, apiKey: "  " });
  assert.equal(bare.mode, "parser");
  assert.equal(bare.modelNote, null);

  const http = await proposePolicy({
    text: "max 1 near a day",
    context,
    apiKey: "key",
    fetchImpl: async () => response({ ok: false, status: 503 }),
  });
  assert.match(http.modelNote, /HTTP 503/);
  assert.equal(http.mode, "parser");

  const prose = await proposePolicy({
    text: "max 1 near a day",
    context,
    apiKey: "key",
    fetchImpl: async () => response({ json: { choices: [{ message: { content: "not json" } }] } }),
  });
  assert.match(prose.modelNote, /did not return JSON/);

  const bad = await proposePolicy({
    text: "max 1 near a day",
    context,
    apiKey: "key",
    fetchImpl: async () => response({
      json: { choices: [{ message: { content: JSON.stringify({ dailyMaxAtomic: "nope" }) } }] },
    }),
  });
  assert.match(bad.modelNote, /atomic integer/);

  const down = await proposePolicy({
    text: "max 1 near a day",
    context,
    apiKey: "key",
    fetchImpl: async () => { throw new Error("offline"); },
  });
  assert.match(down.modelNote, /model call failed/);
});

test("proposePolicy keeps a model object only when it matches the schema", async () => {
  const live = await proposePolicy({
    text: "anything",
    context,
    apiKey: "key",
    fetchImpl: async () => response({
      json: {
        choices: [{
          message: {
            content: JSON.stringify({
              dailyMaxAtomic: "100",
              payees: ["Payee.Testnet"],
              reviewUnknownPayees: true,
            }),
          },
        }],
      },
    }),
  });
  assert.equal(live.mode, "live");
  assert.equal(live.modelNote, null);
  assert.equal(live.proposal.dailyMaxAtomic, "100");
  assert.deepEqual(live.proposal.payees, ["payee.testnet"]);
  assert.equal(live.proposal.reviewUnknownPayees, true);
  assert.equal(validatePolicy(live.proposal).ok, true);
});
