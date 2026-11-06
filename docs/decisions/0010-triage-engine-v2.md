# ADR 0010: Triage engine v2

Status: accepted (Stage B model choice pending the spike, see docs/engine/model-selection.md)

Date: 2026-11-06

## Context

The v1 scanner ran nine string-presence gates over a synthetic ruleset. Its demo envelope reached
`supported/high` with a fabricated 16-hex-digit "address", so a reader of `gates.py` could fairly
call it a form validator. Pillar P2 (ADR-0007) asks for an engine worth paying for: it estimates
whether a security finding is likely to be paid at medium or higher severity, explains why with a
public taxonomy, and never leaks the private doctrine it was compiled from.

Constraints: the scanner and the model adapter stay stdlib-only; the Rust gateway's subprocess
contract (stdin JSON, stdout JSON, exit codes 0/2/1) must keep working unchanged for v1 callers; a
language model never upgrades a verdict past a failing deterministic gate and never moves money
(standing rule S9).

## Decision

1. **Envelope v2** is a strict superset of v1. It adds `hypothesis` (12000 characters),
   `source_ref {repo_url, commit}`, `code_context` (16000) and `program_rules` (8000). v1 envelopes
   map into v2 deterministically. Benchmark rows `{id, title, hypothesis}` map as title plus
   hypothesis; the row id stays out of the content hash.
2. **Stage A** runs a schema 2 ruleset (`rules-v1.public` by default) through six structured,
   fail-closed evaluators: negation-aware phrase matching, precondition-privilege
   classification, quantification, concession detection, a program-rules scope cross-check, and
   evidence-reference format checks. Every gate maps onto one of nine public taxonomy categories
   (docs/engine/taxonomy.md). Stage A can only refuse, hold, or ask for evidence.
3. **Stage B** (`triage-model/`) is an advisory model judge. It calls an OpenAI-compatible
   endpoint (NEAR AI Cloud by default) with a pinned model id, temperature 0, and a strict JSON
   schema. The prompt is versioned and hashed, and the finding travels as one JSON string literal
   inside delimiters it cannot close. A schema violation gets one retry, then fails closed.
   Responses are cached by (envelope hash, ruleset hash, model id, prompt hash). Live calls need
   `APS_LIVE_MODEL=1` and `NEARAI_API_KEY`.
4. **Combiner.** Decisions are ordered `rejected < held < needs_evidence < supported`, and the
   final decision is the minimum of the two stages. Stage B maps its probability through `t_low`
   and `t_high` (defaults 0.35 and 0.65, untuned until prompt 05); a failed Stage B counts as
   `needs_evidence`. The confidence band is `high` only when both stages ran and both say
   `supported`.
5. **Report v2** keeps every v1 field and adds `report_version`, `held_gates`, `stage_a`,
   `stage_b`, `taxonomy_hits`, `model_id`, `model_verifiable`, `prompt_hash`, `thresholds`,
   `score` and `evidence_check`. Free-text fields pass the shared leakage scanner; a leaking model
   rationale is withheld, and any other leak refuses the report.
6. **CLI and SKUs.** A v1 envelope with no arguments still runs the v1 engine and returns the v1
   report byte for byte. A v2 envelope runs Stage A. `--sku triage.v2` adds Stage B. The price
   book gains `triage.v2` at a placeholder 1 USDC until prompt 05 measures the cost per call;
   payment still covers the call, never the verdict.
7. **Private ruleset.** `knowledge-compiler compile-private` compiles `rules-v1.private` from
   `APS_PRIVATE_SOURCES` to a path outside the repository, with opaque gate ids and the same
   public categories, and prints only the manifest hash. The hosted service loads it through
   `APS_RULESET_V2_PATH`.

## Consequences

- The fabricated demo reference now yields `needs_evidence` with the single hit
  `unverified-deployment-evidence`; a well-formed reference is accepted at format level only and
  is reported as `evidence_check: format-only`.
- v1 callers and the Rust API tests are unaffected. Exposing `triage.v2` through the API is
  separate work in the x402 area.
- The penalty factors, thresholds and model choice are placeholders. Prompt 05 owns every number;
  no accuracy claim is made here.
- The model judge is a new dependency on an external, metered service. Cost exhaustion is
  bounded by the per-call price and by the per-caller quotas planned in prompt 08.

## Alternatives considered

- **Model-only verdicts.** Rejected: the model could upgrade a refusal and would make every
  decision depend on an external service.
- **Keep substring gates and add terms.** Rejected: substring tests cannot tell "funds are lost"
  from "no funds are lost".
- **Ship the private ruleset as published digests.** Rejected for the same reason the published
  denylist holds only synthetic stand-ins (`knowledge_compiler/leakage.py`): digests of guessable
  words are reversible.
