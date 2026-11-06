# Scanner Engine Specification (aps-scanner)

This file is the complete build spec for the Python scan engine. Implement exactly what is written here, nothing more, nothing less. All paths are relative to the repository root.

## Product context

A caller pays 1 USDC for a scan of a security finding. The engine treats every input field as hostile data, runs deterministic gates, and returns a decision. It never interprets instructions embedded in the finding, never leaks private taxonomy, and is fully deterministic: same input, same report.

## Layout

- scanner/pyproject.toml: project name aps-scanner, version 0.1.0, requires-python >= 3.10, pytest ini options with testpaths scanner/tests. No runtime deps. Runtime is stdlib only: dataclasses, enum, hashlib, json, re, unicodedata.
- scanner/aps_scanner/__init__.py: public exports.
- scanner/aps_scanner/schema.py: FindingEnvelope dataclass and validation.
- scanner/aps_scanner/normalize.py: normalization and embedded-instruction flagging.
- scanner/aps_scanner/gates.py: nine deterministic gates.
- scanner/aps_scanner/engine.py: pipeline and decision matrix.
- scanner/aps_scanner/report.py: ScanReport and public JSON serialization.
- scanner/README.md: one page, what it is, how to run tests.
- scanner/tests/test_schema.py, test_normalize_injection.py, test_gates.py, test_engine_golden.py.

## Schema: FindingEnvelope

All str fields default to empty string; limits are inclusive maxima:

- title: max 200
- claimed_impact: max 4000
- mechanism: max 4000
- attacker_capability: max 4000
- preconditions: max 4000
- component_class: max 100
- scope_statement: max 4000
- deployed_state_evidence: max 4000
- public_rules_snapshot: max 4000
- known_issue_declaration: Optional str, max 2000, default None
- reproduction_summary: max 4000
- evidence_refs: list of str, each max 300, at most 16, default empty list
- code_excerpt: Optional str, max 4000, default None

validate() returns a list of human-readable error strings; empty list means valid. Raise nothing in validate.

## Normalization

normalize_envelope(env) returns NormalizedEnvelope with the same fields plus contains_embedded_instructions bool:

- NFC-normalize all text fields.
- Replace C0 and C1 control characters with U+FFFD.
- Remove zero-width characters U+200B, U+200C, U+200D, U+FEFF.
- Collapse runs of whitespace to a single space in every field EXCEPT code_excerpt (kept verbatim apart from control-char replacement).
- contains_embedded_instructions is True when ANY field matches any of these patterns, case-insensitive:
  - ignore (all )?(previous|prior) instructions
  - disregard (all )?(previous|prior)
  - system prompt
  - you are now
  - act as (a |an )?(different|new)
  - a fenced code block whose info string contains system (triple backticks followed by a line starting with system)
  - the literal token <|im_start|>
  - TODO:? for the (scanner|model|agent)

FLAGGING ONLY: the flag must never change any gate result or decision. A dedicated test enforces this.

## Gates

Each gate takes the normalized envelope and returns GateResult(status, reason). Statuses: Pass, Fail, NeedsEvidence. Reasons are short public strings with no private terminology. Gate order and names:

1. scope_statement_present: Fail if scope_statement empty, else Pass.
2. attacker_model_present: NeedsEvidence if attacker_capability empty, else Pass.
3. permissionless_reachability: if (attacker_capability + preconditions, lowercased) contains any of admin, owner, operator, privileged, multisig signer, only owner AND preconditions lowercased does not contain permissionless or unprivileged alternative: Fail. Else Pass when attacker_capability non-empty, NeedsEvidence when empty.
4. impact_sink: Fail if claimed_impact empty; NeedsEvidence when claimed_impact non-empty but mentions none of fund, asset, value, loss, theft, drain, mint, freeze (case-insensitive); else Pass.
5. reproduction_non_tautological: Fail if reproduction_summary empty or shorter than 40 characters; NeedsEvidence if it contains none of step, call, transaction, reproduce, 1., 2. (case-insensitive); else Pass.
6. deployed_state_evidence: NeedsEvidence if empty; Pass if it contains an 0x-prefixed hex run of 8 or more chars or the word block followed by a digit; else NeedsEvidence.
7. public_rules_snapshot: NeedsEvidence if empty; Pass if it starts with http:// or https://; else NeedsEvidence.
8. known_issue_declaration: Pass when the declaration is absent; when present and non-empty the decision matrix handles it (held).
9. no_loss_concession: Fail when (reproduction_summary + claimed_impact, lowercased) contains no funds are lost, no fund loss, or no value is extracted AND claimed_impact lowercased mentions any of fund, loss, theft, drain; else Pass.

## Engine: scan(envelope, ruleset_version default rules-v0.synthetic.1) -> ScanReport

- Run schema validation first; on errors raise ScanInputError carrying the error list (the API layer maps it to 400).
- Normalize, run all gates in order, then decide:
  - any Fail -> decision rejected, failed_gates lists them
  - else known_issue_declaration present and non-empty -> held
  - else any NeedsEvidence -> needs_evidence, missing_evidence lists those gates
  - else supported
- confidence_band: high when decision is supported and deployed_state_evidence and public_rules_snapshot are both non-empty; medium when supported with fewer, or needs_evidence; low for rejected or held.

## Report: ScanReport

Fields: decision, confidence_band, missing_evidence (list of gate names), failed_gates (list of gate names), gate_results (list of GateResult), ruleset_version, ruleset_hash, content_hash, contains_embedded_instructions.

content_hash: sha256 hex of canonical JSON of the normalized envelope dict: json.dumps with sort_keys True, separators without spaces, ensure_ascii True, encoded utf-8. ruleset_hash: sha256 hex of the canonical JSON of the compiled ruleset's gates array, pinning the exact ruleset behind every decision.

to_public_json() returns a dict with exactly those fields; to_public_json_string() returns the JSON string. No raw envelope echo, no private markers.

## Rulesets

The nine gates are not hardcoded: they are records in a compiled ruleset
artifact (scanner/rulesets/rules-v0.synthetic.1.json) produced by
aps-knowledge-compiler (../knowledge-compiler) from a knowledge source.
ap-scanner executes the artifact data-driven and fails closed: an
artifact referencing an unknown evaluator, a malformed branch, a
duplicate gate id, or a payload hash mismatch raises RulesetError and the
scan refuses to run. The public repository must always build and test
with the committed sample ruleset; replacing it with real private
knowledge is a compiler-side, leakage-scanned operation.

## Tests (pytest, at least 30 assertions total, deterministic, no network)

- test_schema.py: length limits accept and reject at boundaries, evidence_refs cap of 16, wrong types rejected.
- test_normalize_injection.py: NFC folding (compose a decomposed e-acute), control char replaced, zero-width removed, whitespace collapsed except code_excerpt. INJECTION CORPUS of at least 8 vectors: title injection, fenced system block, code-comment instruction, JSON-embedded string, unicode-obfuscated variant, XML-ish tag, fake TODO, double-encoded block. For every vector: scan of the injected envelope and scan of the same envelope with the injected text removed must produce IDENTICAL decision, confidence_band, and per-gate statuses; only contains_embedded_instructions may differ (True on the injected one).
- test_gates.py: each gate exercised for its Pass, Fail, and NeedsEvidence branches where they exist.
- test_engine_golden.py: supported (all fields strong), needs_evidence (missing deployed_state_evidence), rejected (privileged attacker without permissionless path), held (known_issue_declaration set), rejected (no-loss concession), rejected (reproduction under 40 chars). Differential test: renaming the target protocol in every field keeps decision and gate statuses identical. Privacy test: to_public_json_string() contains no denylisted term and no host-path prefix. Real private terms are checked off-repo only, when APS_PRIVATE_DENYLIST names the operator's private digest file; the published knowledge-compiler/knowledge_compiler/denylist.sha256 holds digests of synthetic stand-ins only, because a published digest of a guessable word is reversible (see the leakage module for the token normalization); a positive control with a synthetic stand-in proves the check is not vacuous. Replay test: same envelope scanned twice gives identical content_hash and identical public JSON.

## Done criteria

cd scanner then python -m pytest -q is all green. Small scoped commits touching only scanner/**. No secrets, no real-world protocol or company names anywhere in the package.

## v2: triage engine (ADR-0010)

Everything above is the v1 specification, and it still holds for v1 envelopes: a v1 envelope sent with no CLI arguments runs the v1 engine and returns the nine-field v1 report, byte for byte. The v2 engine is specified below. The design narrative is in docs/engine/overview.md, and the public categories are in docs/engine/taxonomy.md.

### Envelope v2 (scanner/aps_scanner/envelope_v2.py)

- FindingEnvelopeV2 extends FindingEnvelope with: envelope_version (must be 2), hypothesis (max 12000), source_ref (None or exactly {repo_url max 300, commit max 64}, both non-empty strings), code_context (max 16000), program_rules (max 8000). Every v1 limit still applies. At least one of title, hypothesis, claimed_impact or mechanism must be non-empty.
- A payload is v2 when envelope_version is 2 or any of hypothesis, source_ref, code_context, program_rules is present. envelope_version 1 together with a v2 field is an error. Unknown fields are counted, never named.
- from_v1 copies every v1 field and leaves the v2 fields at their defaults. from_benchmark_row maps {id, title, hypothesis} by truncating title to 200 and hypothesis to 12000. The id never enters the envelope.
- Normalization (normalize_envelope_v2): hypothesis is scrubbed like a v1 text field. program_rules keeps its line structure (CRLF folded to LF, inline whitespace collapsed, blank-line runs collapsed). code_context is verbatim apart from control characters (LF and TAB kept). The source_ref commit is lowercased. The advisory injection flag covers the new fields too.

### Stage A (scanner/aps_scanner/stage_a.py)

- Runs a schema 2 ruleset: rules-v1.public by default, or the file named by APS_RULESET_V2_PATH.
- Schema 2 rules:
  - each gate names exactly one public taxonomy category;
  - a branch may carry a penalty in [0, 1], and a Pass branch never carries one;
  - Held is a legal status;
  - the six structured evaluators are allowed (phrase_affirmed, privilege_required, quantified_impact, concession_present, rules_section_match, evidence_format), with their params validated at load time.
- Decision: any Fail gives rejected; else any Held gives held; else any NeedsEvidence gives needs_evidence; else supported.
- taxonomy_hits are the categories of every non-passing gate, in taxonomy order. penalty_factor is the product of the matched non-passing branches' penalties.

### Stage B contract

- A judge is any callable judge(request) -> dict. The request carries the normalized finding, Stage A's taxonomy hits, the injection flag, the envelope hash and the ruleset hash.
- An ok result carries bountiable_probability, severity_call, taxonomy_hits (public ids only), missing_evidence, public_rationale (max 600), response_digest, model_id, prompt_hash, and optionally model_verifiable.
- The scanner re-validates every result. Anything malformed, and any exception, becomes status fail_closed.

### Combiner (scanner/aps_scanner/combiner.py)

- Order: rejected < held < needs_evidence < supported. The final decision is min(Stage A, Stage B), and equals Stage A when Stage B did not run.
- Stage B decision: probability < t_low gives rejected; probability > t_high gives supported; otherwise needs_evidence. A non-ok status gives needs_evidence. The defaults t_low 0.35 and t_high 0.65 are untuned until prompt 05.
- Confidence band:
  - low for rejected or held;
  - for needs_evidence: low when Stage B failed closed or was unavailable, else medium;
  - for supported: high only when both stages ran and both say supported, else medium.
- Score: Stage B probability times penalty_factor; the penalty_factor alone when Stage B did not run; null when Stage B failed.

### Report v2

- Fields:
  - every v1 field, plus report_version (2) and held_gates;
  - gate_results entries carry category;
  - stage_a {decision, taxonomy_hits, penalty_factor};
  - stage_b {status, decision, bountiable_probability, severity_call, taxonomy_hits, missing_evidence, public_rationale, rationale_withheld, response_digest, failure};
  - taxonomy_hits (the union of both stages), model_id, model_verifiable, prompt_hash, thresholds {t_low, t_high, status}, score, and evidence_check ("format-only").
- Leakage guard: gate ids, reasons, missing-evidence ids, model id, failure code and rationale pass knowledge_compiler.leakage.scan_public_text. A leaking rationale is replaced by "" with rationale_withheld true. Any other leak refuses the report (exit 1). Hash fields are not scanned.

### CLI (python -m aps_scanner)

- Optional arguments: --sku scan.v1|triage.v2 (default scan.v1) and --engine auto|v1|v2 (default auto). A bad command line exits 1 with nothing on stdout.
- A v2 envelope, or --engine v2, runs Stage A only. --sku triage.v2 adds the Stage B judge from the sibling triage-model package (or APS_TRIAGE_MODEL_DIR), mapping a v1 envelope into v2 first. With no configured model, Stage B is unavailable and the decision is at most needs_evidence.
- --engine v1 with v2 input exits 2. Exit codes are otherwise unchanged: 0 report, 2 validation errors, 1 internal error.

### Demo

demo/envelope.json is a v2 envelope. Its deployment reference 0x1234567890abcdef has 16 hex digits, so it is neither an address (40) nor a hash (64). The demo therefore lands on needs_evidence, with the single taxonomy hit unverified-deployment-evidence and confidence medium.
