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

Fields: decision, confidence_band, missing_evidence (list of gate names), failed_gates (list of gate names), gate_results (list of GateResult), ruleset_version, content_hash, contains_embedded_instructions.

content_hash: sha256 hex of canonical JSON of the normalized envelope dict: json.dumps with sort_keys True, separators without spaces, ensure_ascii True, encoded utf-8.

to_public_json() returns a dict with exactly those fields; to_public_json_string() returns the JSON string. No raw envelope echo, no private markers.

## Tests (pytest, at least 30 assertions total, deterministic, no network)

- test_schema.py: length limits accept and reject at boundaries, evidence_refs cap of 16, wrong types rejected.
- test_normalize_injection.py: NFC folding (compose a decomposed e-acute), control char replaced, zero-width removed, whitespace collapsed except code_excerpt. INJECTION CORPUS of at least 8 vectors: title injection, fenced system block, code-comment instruction, JSON-embedded string, unicode-obfuscated variant, XML-ish tag, fake TODO, double-encoded block. For every vector: scan of the injected envelope and scan of the same envelope with the injected text removed must produce IDENTICAL decision, confidence_band, and per-gate statuses; only contains_embedded_instructions may differ (True on the injected one).
- test_gates.py: each gate exercised for its Pass, Fail, and NeedsEvidence branches where they exist.
- test_engine_golden.py: supported (all fields strong), needs_evidence (missing deployed_state_evidence), rejected (privileged attacker without permissionless path), held (known_issue_declaration set), rejected (no-loss concession), rejected (reproduction under 40 chars). Differential test: renaming the target protocol in every field keeps decision and gate statuses identical. Privacy test: to_public_json_string() contains none of redacted, redacted, redacted, redacted, C:\\, /Users/, redacted, redacted, redacted. Replay test: same envelope scanned twice gives identical content_hash and identical public JSON.

## Done criteria

cd scanner then python -m pytest -q is all green. Small scoped commits touching only scanner/**. No secrets, no real-world protocol or company names anywhere in the package.
