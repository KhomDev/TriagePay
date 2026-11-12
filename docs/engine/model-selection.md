# Stage B model selection

Status: **the spike ran on 2026-11-12.** The pinned default stays
`z-ai/glm-5.3-flash`. The figures below are schema compliance, latency, tokens
and cost. They are not accuracy: the spike rows carry no labels, and no
accuracy figure is claimed anywhere.

## What the spike measures (prompt 04, work item 1)

- **Input.** Ten rows from the benchmark's public split (title and
  hypothesis only), extracted outside the repository. The holdout split is
  never read.
- **Measures**, from the API's own usage fields: JSON-schema compliance of
  the first reply and after the single retry; median, p95 and maximum
  latency; prompt and completion tokens per call; cost per call at the
  catalogue price.
- **Spend cap.** USD 0.60 for the run; `triage-model/scripts/model_spike.py`
  stops at 90% of the cap.
- **Request.** Structured output (`json_schema`, strict), temperature 0,
  `reasoning_effort: "low"`, `max_tokens` 4096, client timeout 60 s.
- **Tooling.** The script refuses to run without `APS_LIVE_MODEL=1` and
  `NEARAI_API_KEY`. Its summary records row ids and verdicts, never row text.

## Results (2026-11-12)

Chat models the catalogue (`GET https://cloud-api.near.ai/v1/model/list`)
marked `metadata.verifiable`, read on 2026-11-12. Prices are USD per million
tokens. The flag is the catalogue's claim; this project fetches no
attestation (ADR-0011).

| Model | Provider type | Price in / out | Replies valid | Median latency | Max latency | Completion tokens (median) | Cost per call (mean) |
|---|---|---|---|---|---|---|---|
| `z-ai/glm-5.3-flash` (pinned) | vllm | 0.08 / 0.50 | 10 of 10 | 3.6 s | 6.6 s | 157 | USD 0.00021 |
| `Qwen/Qwen3.8-27B` | vllm | 0.04 / 1.35 | 10 of 10 | 21.5 s | 34.1 s | 1,005 | USD 0.0015 |
| `Qwen/Qwen3.6-35B-A3B-FP8` | vllm | 0.17 / 1.10 | 10 of 10 | 13.4 s | 18.5 s | 2,872 | USD 0.0034 |
| `deepseek-ai/DeepSeek-V4.1-Flash-1017` | vllm | 0.30 / 1.20 | 7 of 10 (3 timeouts) | 16.2 s | 34.7 s | 546 | USD 0.0013 |
| `z-ai/glm-5.3` | tinfoil | 1.80 / 5.75 | 9 of 10 (1 timeout) | 2.5 s | 14.2 s | 198 | USD 0.0040 |

Total spend for the 50 calls: USD 0.0962.

Every reply that arrived was schema-valid on the first attempt. The
timeouts are the client's 60 s limit; those calls failed closed.

## Why the pin stays

`z-ai/glm-5.3-flash` is the only candidate that answered every call, is
the fastest at the median except `z-ai/glm-5.3` (which timed out once and
costs about twenty times more per call), and is the cheapest. It is listed
as TEE-verifiable and served by NEAR's own `vllm` provider type.

The models disagree on the same findings (for example a row one model calls
`medium` at 0.55 and another `info/low` at 0.10). Without labels that says
nothing about which is right; it is one more reason the combiner never lets
Stage B raise a decision above Stage A.

## What the first attempts found

Three problems, all fixed before the run above, each in the client and not
in the key:

1. **`uniqueItems` in the output schema.** NEAR AI Cloud's grammar engine
   refused the whole request: HTTP 400 `Grammar error: Unimplemented keys:
   ["uniqueItems"]`. The keyword is gone from the schema sent to the model;
   `validate_output` still requires distinct items.
2. **Reasoning budget.** Every catalogue model now reasons before it
   answers, and reasoning tokens count against `max_tokens`. With 1024
   tokens and the default effort, all five stopped at the limit with no
   content. The client now sends `reasoning_effort: "low"` and allows 4096
   tokens; the pinned model then needs about 150.
3. **Catalogue drift.** `qwen/qwen3.5-397b-a17b` is no longer listed (HTTP
   400 "not a valid model name"). `Qwen/Qwen3.6-35B-A3B-FP8` and
   `Qwen/Qwen3.8-27B` are new. The verifiable list in
   `triage_model/config.py` was re-read.

## Outcome log

| Date (UTC) | Run | Result | Spend |
|---|---|---|---|
| 2026-11-06 | Probe: 1 public-split row against four models | Every completion HTTP 402: no spending limit configured | USD 0.00 |
| 2026-11-10 | Probe after funding the account: 1 call | Still HTTP 402 `no_limit_configured`: the cap is per API key | USD 0.00 |
| 2026-11-12 | Probe with a capped key: 1 call, 4 tokens | HTTP 200 | under USD 0.0001 |
| 2026-11-12 | Spike, first attempt, 40 calls | 30 HTTP 400 (`uniqueItems`, unknown model id) and 10 empty replies (reasoning budget) | USD 0.00 |
| 2026-11-12 | Spike, 10 rows x 5 models | Table above | USD 0.0962 |
| 2026-11-12 | Cassettes: the demo envelope and the 4 demo findings (synthetic) | Recorded to `triage-model/cassettes/`; replayed by `tests/test_committed_cassettes.py` | about USD 0.001 |

## Cassettes

`triage-model/cassettes/` holds the pinned model's replies for the
repository's synthetic findings: the request digest, the model id, the reply
and the usage. Never the request text, never an auth header. Point
`APS_TRIAGE_CASSETTES` at that directory to replay Stage B without a key. A
change to the prompt, the schema or the default model misses the digest and
fails the replay test, so the cassettes get re-recorded with the change.
