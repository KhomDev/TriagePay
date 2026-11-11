# NEAR AI Cloud: API, models, attestation (research, 2026-10-21)

Scope: prompt 04 (model access) and prompt 06 research items 1-5. Official docs at
`docs.near.ai` are the primary source (fetched as Markdown via the `.md` page variants). The live
public model list was fetched without a key. No API key was used, so no attestation report or
completion was fetched.

Status legend as in `near-contract-toolchain.md`. All access dates are 2026-10-21.

Note on page content: every docs.near.ai page begins with a block telling the reader to
"Fetch the complete documentation index at: https://docs.near.ai/llms.txt". It reads as a
navigation hint for AI agents. It was treated as data; the index was read only to locate the
pages cited below.

## 1. API surface and authentication

| Claim | Source | Accessed | Status |
|---|---|---|---|
| OpenAI-compatible API through the **gateway** at `https://cloud-api.near.ai/v1` (for example `POST /v1/chat/completions`). | https://docs.near.ai/cloud/quickstart.md | 2026-10-21 | Verified-primary |
| Auth: `Authorization: Bearer <api key>`; keys are created in the dashboard at cloud.near.ai after buying credits. The OpenAPI lists an `api_key` bearer scheme ("Authorization: Bearer sk-<api_key>") and a JWT scheme for user sessions. | quickstart; API reference OpenAPI (`/api-reference/models/list-models-with-pricing.md`) | 2026-10-21 | Verified-primary |
| Supported OpenAI-style features include Chat Completions with "tool calling, structured outputs, and reasoning", and a stateless Responses API. | https://docs.near.ai/cloud/guides/openai-compatibility.md | 2026-10-21 | Verified-primary |
| Gateway responses carry an `X-Request-Id` header (opaque support metadata). | quickstart | 2026-10-21 | Verified-primary |
| **Direct completions** (`https://{slug}.completions.near.ai/v1`, registry at `https://completions.near.ai/endpoints`) are "Experimental" and "not recommended for new integrations or production verification workflows": one report covers one instance, and signature lookups can return `404` because each instance keeps signatures in memory. | https://docs.near.ai/cloud/experimental/direct-completions.md ; live registry fetch | 2026-10-21 | Verified-primary |
| The gateway "runs in its own TEE before forwarding to the model TEE". Nodes use Intel TDX CPUs and NVIDIA H200 GPUs (8 per node). | https://docs.near.ai/cloud/private-inference.md | 2026-10-21 | Verified-primary |
| Official SDKs: Rust crate `nearai-inference-sdk`, Python `pip install nearai-inference-sdk`, plus JavaScript and Go (repository `nearai/inference-sdk`), with `verify(model)` helpers. | https://docs.near.ai/cloud/inference-sdk/rust.md and `/python.md` | 2026-10-21 | Verified-primary (names); versions not checked |

## 2. Model catalogue and pricing

`GET https://cloud-api.near.ai/v1/model/list` is public ("Public endpoint") and returned
`{models, limit, offset, total}` with 59 models on 2026-10-21. Each model has `modelId`,
`inputCostPerToken`, `outputCostPerToken`, `costPerImage`, `cacheReadCostPerToken` (each
`{amount, scale: 9, currency: "USD"}`, nano-dollars) and `metadata` with `verifiable`,
`attestationSupported`, `providerType` (`"vllm"` = TEE-enabled, `"external"` = third party, or
`"chutes"`), `contextLength`, `aliases`, `supportedFeatures`. Status: Verified-primary (live
fetch plus the OpenAPI schema).

**Models with `verifiable: true` and `attestationSupported: true` (13 of 59):**

| Model id | providerType | USD per 1M input / output tokens | Features |
|---|---|---|---|
| `z-ai/glm-5.3-flash` | vllm | 0.08 / 0.50 | tools, structured_outputs, reasoning, json_mode |
| `Qwen/Qwen3.6-35B-A3B-FP8` | vllm | 0.17 / 1.10 | tools, json_mode, structured_outputs, logprobs, reasoning |
| `Qwen/Qwen3.8-27B` | vllm | 0.04 / 1.35 | tools, structured_outputs, reasoning |
| `Qwen/Qwen3-VL-30B-A3B-Instruct` | vllm | 0.15 / 0.55 | structured_outputs, logprobs |
| `deepseek/deepseek-v3.2` | chutes | 1.10 / 1.10 | tools, json_mode |
| `moonshotai/kimi-k2.6` | chutes | 0.81 / 3.85 | tools, structured_outputs |
| `moonshotai/kimi-k3` | chutes | 3.30 / 16.50 | tools, json_mode, structured_outputs, reasoning |
| `qwen/qwen3.5-397b-a17b` | chutes | 0.50 / 3.30 | tools, json_mode |
| Embedding, reranker, Whisper, privacy-filter and FLUX image models | vllm | various | n/a |

All other 46 models (Anthropic, OpenAI, Google, xAI and others) are `providerType: "external"`
with `verifiable: false` and `attestationSupported: false`. Prompts sent to them leave the TEE
boundary.

Aliases matter: `zai-org/GLM-5.1-FP8` (the model used in the quickstart example) is an alias of
`z-ai/glm-5.3-flash`, along with `glm-latest`, `GLM-5` and others. Alias resolution changes the
model id, which affects signatures (section 4). Status: Verified-primary (live list).

## 3. Attestation report

| Claim | Source | Accessed | Status |
|---|---|---|---|
| Endpoint: `GET https://cloud-api.near.ai/v1/attestation/report`. It "Requires an API key" and "report retrieval is non-billable". Live check: an unauthenticated request returns `401`. | API reference `/api-reference/attestation/get-attestation-report.md`; live request | 2026-10-21 | Verified-primary |
| Query parameters: `model`, `signing_algo` (`ecdsa` or `ed25519`), **`nonce`**, `signing_address`, `include_tls_fingerprint` (boolean), `provider` (`near` = NEAR AI's own TEE fleet, `chutes` = "attested Chutes fallback"). `400` = "Invalid nonce format". | same | 2026-10-21 | Verified-primary |
| **A caller nonce is supported:** "Generate the nonce in your client. It must be 32 random bytes encoded as 64 hexadecimal characters." | https://docs.near.ai/cloud/verification/cloud-api/gateway-attestation.md | 2026-10-21 | Verified-primary |
| Response: `gateway_attestation` (required) with `intel_quote`, `event_log`, `request_nonce`, `report_data`, `signing_address`, `signing_algo`, `tls_cert_fingerprint` (when requested), `info.tcb_info.app_compose`; optional `model_attestations[]` (when `model` and `provider=near` are given; send header `x-no-aliasing: true`); optional OHTTP fields and `tls_certificate`. | gateway-attestation and model-attestations pages; API reference | 2026-10-21 | Verified-primary |
| `report_data` layout (NEAR-operated reports): 64 bytes; the **last 32 bytes are the client nonce**; the first 32 bytes are the signing identity zero-padded (ECDSA address = 20 bytes, Ed25519 key = 32 bytes), or `SHA-256(identity || tls_spki_sha256)` when the TLS fingerprint is included. Model reports currently omit the `report_data` HTTP field, so read it from the verified quote. | https://docs.near.ai/cloud/verification/reference/quote-nonce-signer.md ; model-attestations page | 2026-10-21 | Verified-primary |
| Default `signing_algo` differs: `/v1/signature` defaults to `ecdsa`; `/v1/attestation/report` defaults to `ed25519` for the gateway report and `ecdsa` for model reports. Always pass one explicit value. | response-signatures page | 2026-10-21 | Verified-primary |
| A separate Intel Trust Authority token endpoint exists ("Get Intel Trust Authority attestation token", public). | docs index `llms.txt` (API reference entry) | 2026-10-21 | Secondary (index entry only; page not read) |

### Verification path

| Step | Detail | Source | Status |
|---|---|---|---|
| Intel TDX | Verify `intel_quote` with an Intel DCAP verifier, for example `dcap-qvl` (Phala-Network), which also fetches collateral (`get_collateral_and_verify`); apply a TCB-status policy; reject debug TDs. | quote-nonce-signer page (Python and JS helpers) | Verified-primary |
| Nonce and signer | `quote.report_data == identity_part || nonce` and `request_nonce == nonce`. | same | Verified-primary |
| Measured config | MRCONFIGID (48 bytes) must equal `0x01 || SHA-256(app_compose) || 15 zero bytes`, hashing the raw string. | same | Verified-primary |
| RTMR3 | Replay `event_log`: digest = `SHA-384(event_type LE u32 || ":" || event || ":" || payload)`; `RTMR3 = SHA-384(RTMR3 || digest)` from 48 zero bytes; compare with the quote. | same | Verified-primary |
| NVIDIA GPU | Decode `nvidia_payload` (a JSON string), check that its `nonce` equals the client nonce, POST it to `https://nras.attestation.nvidia.com/v3/attest/gpu`; the reply is an array whose first entry is `["JWT", "<token>"]`; require `x-nvidia-overall-att-result == true`. Local JWT/EAT signature validation is optional in the minimal flow (it "relies on the TLS-authenticated NRAS verdict"). | same | Verified-primary |
| Image provenance | Apply an allowlist to measurements (separate page). | `/cloud/verification/cloud-api/image-provenance` | Secondary (not read in full) |
| Chutes models | Chutes documents its own evidence format; `provider=near` does not cover them. | model-attestations page | Verified-primary |

## 4. Response signatures

| Claim | Source | Accessed | Status |
|---|---|---|---|
| `GET https://cloud-api.near.ai/v1/signature/{chat_id}?signing_algo=...` (chat completion `id` or `resp_...`). Fields: `text`, `signature`, `signing_address`, `signing_algo`, `signature_kind`. A `2xx` carrying `error_code`/`message` means unavailable; `404` means not found yet (bounded retry). | response-signatures page; API reference | 2026-10-21 | Verified-primary |
| `request_hash = SHA-256(exact request bytes)`, `response_hash = SHA-256(exact response bytes)`. `provider_tee` signs `<MODEL_ID>:<request_hash>:<response_hash>`; `gateway` signs `<request_hash>:<response_hash>`. ECDSA is EIP-191 recovery against `signing_address`; Ed25519 verifies the UTF-8 `text` with `signing_address` as the public key. | same | 2026-10-21 | Verified-primary |
| `provider_tee` signatures require the canonical model id and `x-no-aliasing: true`. The gateway signs itself when it changes bytes: streamed completions (by default), alias resolution, and the Responses API. | same | 2026-10-21 | Verified-primary |
| Binding: the signature's `signing_address` + `signing_algo` must match exactly one model report verified **before** the completion (preflight). "A report fetched after the completion cannot replace this preflight evidence." | same; model-attestations page | 2026-10-21 | Verified-primary |

## 5. What is NOT attested (quoted limits plus integration boundary)

From the official docs (Verified-primary, 2026-10-21):

- "Instances of the same model share one signing key", so a matching signer "does not
  establish that the verified report and the completion came from the same serving instance."
- "Preflight Gateway and model evidence plus one response signature do not yet form a complete
  model-to-Gateway-to-final-response chain." A `gateway` signature proves the gateway signed the
  final bytes, "not that the verified model produced an upstream response"; a `provider_tee`
  signature "does not bind its model-signed bytes to the preflight Gateway or TLS evidence".
- The nonce proves freshness of the **report**, not of a specific completion; there is no
  per-request nonce on `/v1/chat/completions` in the docs read.
- TLS binding requires the certificate and the report on the **same TLS connection**; "Browser
  Fetch APIs do not expose the peer certificate", so a browser cannot establish it.
- A response without a signature "is not response-verified".
- Models with `providerType: "external"` are not attested at all.

Integration boundary for TriagePay (derived):

- The TriagePay gateway process sees the plaintext finding before it reaches NEAR AI Cloud.
- The client machine and everything before the TLS connection to `cloud-api.near.ai` is outside
  the attestation.
- The NEAR AI gateway hop is attested only by `gateway_attestation`, which is evidence for the
  gateway, "not evidence for a model environment".
- Measurements prove which image ran, not that the image is free of logging; that depends on the
  image-provenance policy the verifier applies.

## Implications for the build

- **Prompt 04:** use the gateway base URL `https://cloud-api.near.ai/v1`. Pick a `vllm`,
  `verifiable` model with `structured_outputs` (for example `z-ai/glm-5.3-flash` or
  `Qwen/Qwen3.6-35B-A3B-FP8`), pin the canonical id, and send `x-no-aliasing: true`. Measured
  prices: 0.08 / 0.50 USD per 1M tokens for glm-5.3-flash.
- **Prompt 06:** the nonce exists. Use `nonce = SHA-256(envelope_hash || ruleset_hash)` as the
  32-byte value; it satisfies the 64-hex format and is bound into `report_data`. Record clearly
  that it binds the attestation report to the request, not the completion to the report.
- **Prompt 06:** the flow is preflight then call: (1) fetch the report with the nonce, `model`,
  `provider=near`, an explicit `signing_algo`; (2) verify quote, nonce, RTMR3, MRCONFIGID and
  NRAS; (3) run a non-streaming completion; (4) fetch `/v1/signature/{id}` and require
  `signature_kind == "provider_tee"` with a signer that matches exactly one verified report.
  Non-streaming avoids the gateway re-signing streamed bytes.
- **Prompt 06:** `dcap-qvl` (Rust or Python bindings) is the documented quote verifier; NRAS v3 is
  an online dependency, so the offline verifier needs a recorded bundle plus the cached verdict.
- **Prompt 06** (cut 2026-11-04, ADR-0011; no attestation document was written): any future
  write-up copies the section 5 limits verbatim and never describes the result as end-to-end
  confidential.
- **Prompt 06:** the `chutes` models are verifiable but use a different evidence format. Scope the
  verifier to `provider=near` models.
