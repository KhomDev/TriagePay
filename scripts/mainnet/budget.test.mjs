import test from "node:test";
import assert from "node:assert/strict";
import { createCostYocto, projectBudget, stakeYocto, WALLET_CODE_BYTES } from "./budget.mjs";

test("the stake is code plus state plus the working balance, and the fee sits on top", () => {
  const stake = stakeYocto();
  const code = WALLET_CODE_BYTES * 10_000_000_000_000_000_000n;
  assert.equal(stake, code + 1_000n * 10_000_000_000_000_000_000n + 300_000_000_000_000_000_000_000n);
  assert.equal(createCostYocto(), stake + 1_000_000_000_000_000_000_000_000n);
});

test("a factory the size of the wasm-opt build stays under 6.5 NEAR", () => {
  const budget = projectBudget(250_000n);
  assert.equal(budget.fits, true);
  assert.equal(budget.secondWallet, false);
  assert.ok(budget.peak <= budget.ceiling);
});

test("an unpacked second copy of the wallet on the factory blows the ceiling", () => {
  const budget = projectBudget(400_000n);
  assert.equal(budget.fits, false);
});
