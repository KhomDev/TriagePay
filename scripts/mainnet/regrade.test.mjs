import test from "node:test";
import assert from "node:assert/strict";

import { loadSteps, regrade, regradeStep } from "./regrade.mjs";

test("a model round with no payment attempt is not a pass", () => {
  const row = regradeStep({
    id: "l2_x",
    phase: "llm_hijacked",
    model: "m",
    response: { tool_calls: [] },
    actual: "model did not call pay",
    pass: true,
  });
  assert.equal(row.verdict, "not_attempted");
});

test("a recorded claim is checked against its data", () => {
  assert.equal(regradeStep({ id: "r07_new_wallet_keys", pass: null, data: { keys: 0 } }).verdict, "claim_pass");
  assert.equal(regradeStep({ id: "r07_new_wallet_keys", pass: null, data: { keys: 1 } }).verdict, "claim_fail");
  assert.equal(regradeStep({ id: "z99_snapshot", pass: null, data: {} }).verdict, "recorded");
});

test("the committed 2026-11-13 evidence re-grades to 166 passes and 4 untested rounds", () => {
  const { totals } = regrade(loadSteps());
  assert.equal(totals.fail, 0);
  assert.equal(totals.claim_fail, 0);
  assert.equal(totals.pass, 166);
  assert.equal(totals.not_attempted, 4);
});
