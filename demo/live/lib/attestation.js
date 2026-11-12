// What the hosted demo can check on a NEAR AI Cloud attestation report.
//
// The HTTP report echoes a client nonce, and the gateway `report_data`
// field repeats that nonce. Those two checks are consistency checks of the
// JSON the API returned. They are not a verification of the Intel TDX
// quote. This module does not run DCAP, replay RTMR3, or check MRCONFIGID.

import { randomBytes } from "node:crypto";

export const ATTESTATION_URL = "https://cloud-api.near.ai/v1/attestation/report";
export const DEFAULT_MODEL = "z-ai/glm-5.3-flash";

export function freshNonce() {
  return randomBytes(32).toString("hex");
}

function hex(value) {
  return String(value || "").trim().toLowerCase().replace(/^0x/, "");
}

export function assessReport(report, nonce) {
  const expected = hex(nonce);
  const gateway = report && report.gateway_attestation;
  const models = Array.isArray(report && report.model_attestations) ? report.model_attestations : [];
  const checks = [];

  const gatewayNonce = gateway && hex(gateway.request_nonce) === expected && expected.length === 64;
  checks.push({
    name: "gateway_request_nonce",
    verified: Boolean(gatewayNonce),
    detail: gatewayNonce
      ? "The gateway report echoes the nonce this server just generated."
      : "The gateway report did not echo this server's nonce.",
  });

  const reportData = gateway ? hex(gateway.report_data) : "";
  const tailMatches = reportData.length >= 64 && reportData.slice(-64) === expected;
  const address = gateway ? hex(gateway.signing_address) : "";
  const headMatches = gateway && gateway.signing_algo === "ed25519"
    && address.length === 64
    && reportData.slice(0, 64) === address;
  checks.push({
    name: "gateway_report_data",
    verified: Boolean(tailMatches),
    detail: tailMatches
      ? `The gateway report_data ends with the nonce${headMatches ? " and starts with the reported ed25519 signing address" : ""}. This matches the HTTP fields. It is not a check of the Intel quote.`
      : "The gateway report_data did not end with this server's nonce.",
  });

  const quotePresent = Boolean(gateway && gateway.intel_quote && String(gateway.intel_quote).length > 32);
  checks.push({
    name: "intel_quote",
    verified: false,
    detail: quotePresent
      ? "An Intel quote is in the response. This server does not verify it: no DCAP collateral, no RTMR3 replay, no MRCONFIGID check."
      : "The response did not include an Intel quote to verify.",
  });

  if (models.length === 0) {
    checks.push({
      name: "model_request_nonce",
      verified: false,
      detail: "No model attestation was returned, so the model environment is not checked.",
    });
  } else {
    const matched = models.filter((item) => hex(item && item.request_nonce) === expected);
    checks.push({
      name: "model_request_nonce",
      verified: matched.length === models.length && models.length > 0,
      detail: matched.length === models.length
        ? `All ${models.length} model report(s) echo the nonce. Model reports omit report_data; the quote binding is not verified here.`
        : "A model report nonce does not match.",
    });
    checks.push({
      name: "model_quote",
      verified: false,
      detail: "Model quotes are not verified in this demo. A listed TEE model is not, by itself, an attested verdict.",
    });
  }

  checks.push({
    name: "response_signature",
    verified: false,
    detail: "No completion was signed for this check. A response signature needs a paid non-streaming call and is not fetched here.",
  });

  const consistent = checks
    .filter((item) => item.name === "gateway_request_nonce" || item.name === "gateway_report_data")
    .every((item) => item.verified);
  return {
    consistent,
    quoteVerified: false,
    model: models[0] && models[0].model_name ? models[0].model_name : null,
    checks,
    summary: consistent
      ? "The attestation HTTP body is bound to a nonce I generated. The Intel quote is not verified, and no model response was signed."
      : "The attestation HTTP body did not match the nonce I generated. Nothing here is verified.",
  };
}

export async function fetchReport({ nonce, model = DEFAULT_MODEL, fetchImpl = fetch, apiKey = "" }) {
  const url = new URL(ATTESTATION_URL);
  url.searchParams.set("nonce", nonce);
  url.searchParams.set("signing_algo", "ed25519");
  url.searchParams.set("model", model);
  url.searchParams.set("provider", "near");
  url.searchParams.set("include_tls_fingerprint", "false");
  const headers = { accept: "application/json", "x-no-aliasing": "true" };
  if (apiKey) headers.authorization = `Bearer ${apiKey}`;
  const response = await fetchImpl(url, { headers, signal: AbortSignal.timeout(20000) });
  const text = await response.text();
  let body = null;
  try {
    body = JSON.parse(text);
  } catch {
    body = null;
  }
  if (!response.ok || !body) {
    const message = body && body.error && body.error.message
      ? body.error.message
      : `attestation HTTP ${response.status}`;
    const error = new Error(message);
    error.status = response.status;
    throw error;
  }
  return body;
}
