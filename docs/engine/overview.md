# Triage engine v2: overview

The engine decides, for one security finding, whether it is likely to be paid at medium or higher
severity, and explains its answer in the public taxonomy (docs/engine/taxonomy.md). Design record:
ADR-0010. No accuracy figure is claimed anywhere in this document; the benchmark evaluation
(prompt 05) owns every number.

## Pipeline

```
envelope (v1 or v2) -> validate -> normalize -> Stage A (deterministic gates)
                                             -> Stage B (model judge, optional, advisory)
                                             -> combiner -> report v2 -> leakage guard -> JSON
```

| Component | Location | Role |
|---|---|---|
| Envelope v2 | `scanner/aps_scanner/envelope_v2.py` | v1 superset; v1 and benchmark-row mappings |
| Stage A | `scanner/aps_scanner/stage_a.py`, `textmatch.py`, `ruleset.py` | structured gates from a compiled schema 2 ruleset |
| Public ruleset | `scanner/rulesets/rules-v1.public.json` (source in `knowledge-compiler/samples/public-v1/`) | nine gates, one per public category |
| Stage B | `triage-model/triage_model/` | prompt, client, schema validation, retry, cache, cassettes |
| Combiner | `scanner/aps_scanner/combiner.py` | minimum of the stages; thresholds; confidence; score |
| Report v2 and guard | `scanner/aps_scanner/engine_v2.py`, `public_guard.py` | public JSON; leakage scan of free-text fields |
| CLI bridge | `scanner/aps_scanner/__main__.py` | stdin/stdout contract, `--sku triage.v2` |
| Benchmark adapter | `triage-model/bench_adapter.py` | `tool_input.jsonl` to `tool_output.jsonl` |

## Stage A

Stage A evaluates the gates of a schema 2 ruleset. Each gate names one public category; each
branch has a status (`Pass`, `Fail`, `Held`, `NeedsEvidence`), a public reason, and an optional
penalty factor. The decision is `rejected` on any Fail, else `held` on any Held, else
`needs_evidence` on any NeedsEvidence, else `supported`. "Supported" from Stage A means only "no
deterministic objection".

The evaluators are registered in a fixed, fail-closed registry. An unknown evaluator, a malformed
parameter, an unknown category or a tampered payload hash refuses to load.

| Evaluator | What it does |
|---|---|
| `phrase_affirmed` | phrase match on token boundaries, ignoring occurrences negated within four tokens in the same clause |
| `privilege_required` | role words in `attacker_capability` or requirement phrases in the context, cancelled by an affirmed permissionless escape |
| `quantified_impact` | an amount, percentage, token quantity or absolute phrase within twelve tokens of an affirmed impact verb |
| `concession_present` | a concession phrase in a sentence with no rebuttal marker |
| `rules_section_match` | overlap between a marked section of `program_rules` and the component or title |
| `evidence_format` | placeholder, well-formed on-chain reference, or pinned source; format level only |

## Stage B

Stage B is advisory. It sees the normalized finding, Stage A's public categories, the
embedded-instruction flag and the cache-key hashes.

- **Prompt.** The template is versioned (`triage-prompt-v1`) and hashed. Its hash,
  `ef26db4df39e9ee4417e9727b32f950e22f52d80ba543029af4313a8a9a494a1`, is recorded in every report.
  The finding is canonical JSON, encoded again as one JSON string literal with `<` and `>`
  escaped. It is placed between `<<<FINDING_DATA_BEGIN>>>` and `<<<FINDING_DATA_END>>>`, so it can
  never close the block. The system prompt declares the block inert.
- **Output schema.** The reply must carry exactly these fields: `bountiable_probability` (0..1),
  `severity_call` (`invalid`, `info/low`, `medium`, `high`), `taxonomy_hits` (public ids only),
  `missing_evidence` (a fixed enum) and `public_rationale` (at most 600 characters).
- **Failure handling.** One retry on a schema violation, then `fail_closed`. A timeout or a
  transport error fails closed at once. The scanner re-validates whatever the judge returns.
- **Live switch.** Live calls need `APS_LIVE_MODEL=1` and `NEARAI_API_KEY`. The key is read from
  the environment and never logged.
- **Model.** The model id is pinned and configurable through `APS_TRIAGE_MODEL_ID`. Every report
  records `model_verifiable`, the catalogue's TEE-verifiable flag for that id. The default must
  be verifiable.

## Combiner

| Stage A \ Stage B | rejected | needs_evidence | supported | not run |
|---|---|---|---|---|
| rejected | rejected | rejected | rejected | rejected |
| held | rejected | held | held | held |
| needs_evidence | rejected | needs_evidence | needs_evidence | needs_evidence |
| supported | rejected | needs_evidence | supported | supported |

The full 4x4 table over all four decisions is proven exhaustively in
`scanner/tests/test_combiner.py`. Stage B never produces `held`. Its probability maps through
`t_low = 0.35` and `t_high = 0.65`; these defaults are untuned until prompt 05.

- **Confidence.** `high` only when both stages ran and agree on `supported`. `medium` for a Stage
  A-only `supported`, or for a `needs_evidence` with a working or absent Stage B. `low` otherwise.
- **Score (benchmark).** The Stage B probability times the Stage A penalty factor. In Stage A-only
  mode it is the penalty factor alone. It is absent (an unpredicted row) when Stage B failed
  closed.

## Determinism and cost model

- The same envelope, ruleset and judge output always produce the same report. `content_hash`
  pins the normalized v2 envelope, and `ruleset_hash` pins the gates.
- Valid Stage B results are cached by (envelope hash, ruleset hash, model id, prompt hash), so a
  replay is bit-identical and makes no model call.
- Cassettes (`triage-model/cassettes/`) store a request digest, the model id, the reply content
  and the usage, never the request text or any auth header. Only synthetic or public-benchmark
  inputs may be recorded.
- Cost per call is the API's usage fields times the catalogue price. It is unmeasured until the
  spike runs (docs/engine/model-selection.md). The `triage.v2` price is a 1 USDC placeholder.

## Threat model rows

| Threat | Vector | Mitigation in this engine | Residual risk |
|---|---|---|---|
| Prompt injection | Finding text instructs the model to approve it | Finding passed as one escaped JSON string inside delimiters it cannot close; system prompt declares it inert; the injection flag is passed to the model and surfaced in the report; the combiner caps the final decision at Stage A, so even an obedient model cannot raise a decision (tests run an obedient fake model over an injection corpus) | A model can still be talked into a lower or noisier verdict for an injected finding |
| Model drift | The provider changes weights or alias resolution behind a model name | Pinned canonical model id with `x-no-aliasing: true`; prompt hash and model id in every report; cassette replay detects a changed prompt (a digest miss fails closed) | Silent weight changes under the same id are detectable only by re-recording and comparing cassettes |
| Cost exhaustion | Many triage.v2 calls drain the model budget | Payment covers each call before work starts; the cache answers replays without a model call; bounded `max_tokens`; one retry at most | Per-caller quotas are planned in prompt 08 and not yet built |
| Rationale leakage | The model echoes private or identifying text in `public_rationale` | Rationale capped at 600 characters, generic-only instructions, and scanned by the shared leakage scanner (denylist digests plus host-path, URL, account-id and secret shapes); a hit withholds the rationale and sets `rationale_withheld` | The guard only knows patterns on its lists; novel identifying text can pass |
