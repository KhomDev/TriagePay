# What the demo verifies about NEAR AI Cloud

The paid scan route still does not fetch an attestation. ADR-0011 stands
for that route: a verdict is not described as attested.

The hosted demo has a separate button, Check attestation. It calls
`GET https://cloud-api.near.ai/v1/attestation/report` with a 32-byte nonce
I generate, `signing_algo=ed25519`, `provider=near`, and the model
`z-ai/glm-5.3-flash` (or `APS_TRIAGE_MODEL_ID`). No API key is required for
that report. `NEARAI_API_KEY`, when set, is sent only as `Authorization`.
The demo does not send a chat completion, so it does not fetch
`/v1/signature/{id}`.

On 2026-11-12 an unauthenticated report came back in a few hundred
milliseconds. The gateway object echoed `request_nonce`, and
`report_data` was 64 bytes: the reported ed25519 signing address, then the
nonce. The model object echoed the same nonce and did not include
`report_data`.

`demo/live/lib/attestation.js` checks only that:

- `gateway_attestation.request_nonce` equals the nonce just generated.
- `gateway_attestation.report_data` ends with that nonce. For ed25519 it
  also checks that the field starts with `signing_address`.
- Every `model_attestations[].request_nonce` equals the same nonce, when
  any model report is present.

Those are consistency checks of the JSON body. They are not a verification
of the Intel TDX quote. This repository does not run DCAP, does not replay
`event_log` into RTMR3, does not check MRCONFIGID, and does not call the
NVIDIA attestation service. A quote that is present is reported as present
and unverified.

A response signature is not fetched. The official docs say a report fetched
without a matching preflight-and-signature chain does not prove that a
particular completion ran inside the measured image. The button's summary
says that in one sentence.

If the endpoint returns 429 or does not answer, the panel says nothing was
verified. It does not invent a pass.
