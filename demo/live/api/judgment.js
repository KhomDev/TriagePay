import { assessReport, fetchReport, freshNonce } from "../lib/attestation.js";
import { fromError, json } from "../lib/http.js";
import { loadConfig } from "../lib/config.js";
import { assertModelBudget } from "../lib/guard.js";

export async function GET(request) {
  const nonce = freshNonce();
  const model = process.env.APS_TRIAGE_MODEL_ID || "z-ai/glm-5.3-flash";
  try {
    // The attestation request uses the operator's key: count it first
    // (red-team RT-05).
    await assertModelBudget(request);
    const report = await fetchReport({
      nonce,
      model,
      apiKey: process.env.NEARAI_API_KEY || "",
    });
    const assessment = assessReport(report, nonce);
    const config = loadConfig();
    return json(200, {
      ok: true,
      network: config.network,
      model,
      nonce,
      consistent: assessment.consistent,
      quoteVerified: false,
      summary: assessment.summary,
      checks: assessment.checks,
    });
  } catch (error) {
    const status = error && error.status;
    if (status === 429) {
      return json(429, {
        ok: false,
        consistent: false,
        quoteVerified: false,
        summary: "NEAR AI Cloud rate-limited the attestation request. Nothing was verified.",
        checks: [],
      });
    }
    if (error && error.message && /attestation HTTP|fetch failed|timeout|aborted/i.test(error.message)) {
      return json(200, {
        ok: false,
        consistent: false,
        quoteVerified: false,
        summary: "The attestation endpoint did not answer. Nothing was verified. The paid scan route still does not fetch a report.",
        checks: [],
      });
    }
    return fromError(error);
  }
}
