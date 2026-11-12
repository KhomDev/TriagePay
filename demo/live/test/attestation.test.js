import test from "node:test";
import assert from "node:assert/strict";
import { assessReport } from "../lib/attestation.js";

const NONCE = "ab".repeat(32);
const ADDRESS = "cd".repeat(32);

function report(overrides = {}) {
  return {
    gateway_attestation: {
      signing_address: ADDRESS,
      signing_algo: "ed25519",
      request_nonce: NONCE,
      report_data: ADDRESS + NONCE,
      intel_quote: "aa".repeat(80),
      ...overrides.gateway,
    },
    model_attestations: overrides.models || [
      { request_nonce: NONCE, model_name: "z-ai/glm-5.3-flash", intel_quote: "bb".repeat(40) },
    ],
  };
}

test("a matching nonce is a consistency check, not a verified quote", () => {
  const result = assessReport(report(), NONCE);
  assert.equal(result.consistent, true);
  assert.equal(result.quoteVerified, false);
  const byName = Object.fromEntries(result.checks.map((item) => [item.name, item.verified]));
  assert.equal(byName.gateway_request_nonce, true);
  assert.equal(byName.gateway_report_data, true);
  assert.equal(byName.intel_quote, false);
  assert.equal(byName.model_quote, false);
  assert.equal(byName.response_signature, false);
  assert.match(result.summary, /Intel quote is not verified/);
});

test("a mismatched nonce fails the consistency check", () => {
  const result = assessReport(report({ gateway: { request_nonce: "00".repeat(32) } }), NONCE);
  assert.equal(result.consistent, false);
  assert.match(result.summary, /did not match/);
});

test("a missing model report is recorded as unchecked", () => {
  const result = assessReport(report({ models: [] }), NONCE);
  const model = result.checks.find((item) => item.name === "model_request_nonce");
  assert.equal(model.verified, false);
});
