import test from "node:test";
import assert from "node:assert/strict";
import fc from "fast-check";
import { decimalToAtomic, parsePolicyText, validatePolicy } from "../lib/policy-nl.js";

const FIELDS = ["dailyMaxAtomic", "perCallMaxAtomic", "payees", "approvalThresholdAtomic", "reviewUnknownPayees"];

function atomicOf(whole, frac, decimals) {
  const padded = String(frac).slice(0, decimals).padEnd(decimals, "0");
  return BigInt(whole) * 10n ** BigInt(decimals) + (padded ? BigInt(padded) : 0n);
}

test("decimalToAtomic is the truncated integer at the given scale", () => {
  fc.assert(
    fc.property(
      fc.integer({ min: 0, max: 18 }),
      fc.integer({ min: 0, max: 1_000_000 }),
      fc.integer({ min: 0, max: 999_999_999 }),
      (decimals, whole, frac) => {
        const text = decimals === 0 ? String(whole) : `${whole}.${frac}`;
        const parsed = decimalToAtomic(text, decimals);
        assert.equal(parsed, atomicOf(whole, frac, decimals).toString());
        assert.match(parsed, /^[0-9]+$/);
      },
    ),
  );
  fc.assert(
    fc.property(fc.string(), (text) => {
      const parsed = decimalToAtomic(text, 24);
      if (parsed != null) assert.match(parsed, /^[0-9]+$/);
    }),
  );
});

test("the policy parser never emits an out-of-schema proposal", () => {
  const context = {
    decimals: fc.constantFrom(0, 6, 18, 24),
    triagePayee: fc.constantFrom("payee.testnet", "not an account", ""),
  };
  fc.assert(
    fc.property(fc.string({ maxLength: 240 }), context.decimals, context.triagePayee, (text, decimals, triagePayee) => {
      const parsed = parsePolicyText(text, { decimals, triagePayee });
      assert.equal(parsed.mode, "parser");
      assert.deepEqual(Object.keys(parsed.proposal).sort(), [...FIELDS].sort());
      const checked = validatePolicy(parsed.proposal);
      assert.equal(checked.ok, true, checked.reason);
      for (const key of ["dailyMaxAtomic", "perCallMaxAtomic", "approvalThresholdAtomic"]) {
        const value = parsed.proposal[key];
        if (value != null) assert.match(value, /^[0-9]{1,40}$/);
      }
      if (parsed.proposal.payees != null) {
        assert.ok(parsed.proposal.payees.length <= 8);
        for (const id of parsed.proposal.payees) assert.match(id, /^[a-z0-9._-]+$/);
      }
      if (parsed.proposal.reviewUnknownPayees != null) {
        assert.equal(typeof parsed.proposal.reviewUnknownPayees, "boolean");
      }
    }),
  );
});

test("a sentence that names a cap, a payee, and a hold stays inside the schema", () => {
  fc.assert(
    fc.property(
      fc.integer({ min: 0, max: 20 }),
      fc.integer({ min: 0, max: 999 }),
      fc.constantFrom("near", "wnear"),
      (whole, frac, unit) => {
        const text = `max ${whole}.${frac} ${unit} a day, max 1 ${unit} per call, only pay payee.testnet, ask me above 0.5, hold unknown payees`;
        const parsed = parsePolicyText(text, { decimals: 6, triagePayee: "api.testnet" });
        assert.equal(validatePolicy(parsed.proposal).ok, true);
        assert.equal(parsed.proposal.reviewUnknownPayees, true);
        assert.deepEqual(parsed.proposal.payees, ["payee.testnet"]);
        assert.match(parsed.proposal.dailyMaxAtomic, /^[0-9]{1,40}$/);
        assert.equal(parsed.proposal.perCallMaxAtomic, "1000000");
      },
    ),
  );
  const huge = parsePolicyText("max " + "9".repeat(50) + " near a day", { decimals: 24, triagePayee: "api.testnet" });
  assert.equal(huge.proposal.dailyMaxAtomic, null);
  assert.equal(validatePolicy(huge.proposal).ok, true);
});
