# ADR 0011: Attested verdicts are cut

Status: accepted

Date: 2026-11-04 (recorded 2026-11-11; the number was reserved and left
empty until now)

## Context

Pillar P7 of ADR-0007 planned attested verdicts: run the Stage B model on
NEAR AI Cloud inside a TEE, fetch the attestation with a fresh nonce,
verify it, and bind the verified evidence to each report. The research is
in docs/research/near-ai-cloud.md.

The build-out fell three days behind in the first week of November, and
ADR-0007 fixes the cut order: P8 (NEAR Intents deposits) first, then P7.

## Decision

P7 is cut. No attestation is fetched or verified anywhere in this
repository.

What remains of it is a recorded flag, not a proof: every v2 report
carries `model_verifiable`, the model catalogue's TEE flag for the
configured model id (docs/engine/overview.md).

## Consequences

- The project is not submitted to the `privacy` track (ADR-0007: the
  privacy claim must rest on a demonstrated mechanism).
- No document may describe a verdict as attested, confidential or
  TEE-verified. "The configured model is listed as TEE-verifiable" is the
  strongest true statement.
- Stage B itself has not run against the live service: every completion
  attempt up to 2026-11-11 was refused with HTTP 402 because the API key
  has no spending limit (docs/engine/model-selection.md).
