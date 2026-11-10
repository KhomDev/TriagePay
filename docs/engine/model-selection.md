# Stage B model selection

Status: **the spike is pending account credits.** No latency, token or cost figure has been
measured, and none is stated here.

## What the spike measures (prompt 04, work item 1)

- **Candidates.** `deepseek-ai/DeepSeek-V4.1-Flash-1017` against one stronger TEE-verifiable
  model (`z-ai/glm-5.3` or `qwen/qwen3.5-397b-a17b`). The current pinned default,
  `z-ai/glm-5.3-flash`, is measured alongside them.
- **Input.** Ten rows from the benchmark's public split, stratified five bountiable and five not.
  They are extracted outside the repository. The holdout split is never read for this.
- **Measures**, taken from the API's own usage fields:
  - JSON-schema compliance of the first reply, and after the single retry;
  - median, p95 and maximum latency;
  - prompt and completion tokens per call;
  - cost per call and per finding, at the catalogue price.
- **Spend cap.** USD 2.00 in total. `triage-model/scripts/model_spike.py` stops at 90% of the cap.
- **Tooling.** The script refuses to run without `APS_LIVE_MODEL=1` and `NEARAI_API_KEY`. Its
  summary records row ids and verdicts, never row text.

## Candidates (public catalogue)

Chat models that `GET https://cloud-api.near.ai/v1/model/list` marks `metadata.verifiable`
(TEE-backed). Prices are USD per million tokens, converted from the catalogue's per-token amounts
(scale 9), as read on 2026-11-06.

| Model id | Input | Output | Role in the spike |
|---|---|---|---|
| `deepseek-ai/DeepSeek-V4.1-Flash-1017` | 0.30 | 1.20 | candidate |
| `z-ai/glm-5.3` | 1.80 | 5.75 | stronger candidate |
| `qwen/qwen3.5-397b-a17b` | 0.50 | 3.30 | stronger candidate (alternative) |
| `z-ai/glm-5.3-flash` | 0.08 | 0.50 | current pinned default |
| `moonshotai/kimi-k2.6` | 0.81 | 3.85 | not tested |
| `moonshotai/kimi-k3` | 3.30 | 16.50 | not tested |

Other catalogue entries, including the proxied third-party models, are not TEE-verifiable. They
are excluded because the privacy story needs a verifiable model. Every report records
`model_verifiable` next to `model_id`.

## Outcome log

| Date (UTC) | Run | Result | Spend |
|---|---|---|---|
| 2026-11-06 | Probe: 1 public-split row against each of `DeepSeek-V4.1-Flash-1017`, `z-ai/glm-5.3`, `z-ai/glm-5.3-flash`, `qwen/qwen3.5-397b-a17b` | Every chat completion returned HTTP 402. The account has no spending limit configured, so completions are refused before inference. The model list endpoint answers normally. | USD 0.00 |
| 2026-11-10 | Probe after the account was funded: 1 call (4 output tokens) to `z-ai/glm-5.3-flash` | Still HTTP 402 `no_limit_configured`. Funding the account alone does not enable completions: NEAR AI Cloud applies a spend cap per API key, and this key has none. The spike waits for a key with a cap. | USD 0.00 |

## Current pin

`z-ai/glm-5.3-flash` stays pinned as `triage_model.config.DEFAULT_MODEL_ID`. The reasons:

- the catalogue marks it verifiable;
- it lists structured outputs;
- it is the cheapest verifiable chat model in the table.

This is a provisional choice made from catalogue metadata only. Its schema compliance, latency and
cost are unmeasured. Once credits are active, the spike decides whether the pin stays, and this
file records the measured results and the recommendation.
