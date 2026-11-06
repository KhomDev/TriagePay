# aps-scanner

Deterministic scan engine for paid security-finding triage. A caller pays one
unit per scan of a submitted security finding; this package decides whether
that finding is worth a human look.

The engine treats every input field as hostile data. It never interprets
instructions embedded in a finding: such text is flagged, and the flag is
advisory only and never changes a gate result or the decision. The engine
never echoes the submitted envelope back.

## Layout

- `pyproject.toml` - package metadata, pytest and ruff configuration
- `aps_scanner/schema.py` - `FindingEnvelope` dataclass and validation
- `aps_scanner/normalize.py` - normalization and injection flagging
- `aps_scanner/ruleset.py` - compiled-ruleset loading and data-driven evaluation (fail closed)
- `aps_scanner/gates.py` - the nine deterministic gates, executed from the ruleset
- `aps_scanner/engine.py` - pipeline and decision matrix
- `aps_scanner/report.py` - `ScanReport` and public JSON serialization
- `aps_scanner/envelope_v2.py` - envelope v2 (v1 superset) and the v1 / benchmark-row mappings
- `aps_scanner/textmatch.py` - Stage A text primitives (negation, privilege, quantity, concession, scope, evidence format)
- `aps_scanner/stage_a.py` - Stage A of the v2 engine over a schema 2 ruleset
- `aps_scanner/combiner.py` - minimum-of-stages combiner, thresholds, confidence, score
- `aps_scanner/engine_v2.py` - `triage()` and the v2 report
- `aps_scanner/public_guard.py` - leakage guard for the v2 report
- `aps_scanner/stage_b_bridge.py` - loads the Stage B judge from ../triage-model for `--sku triage.v2`
- `aps_scanner/taxonomy.py` - the public category ids
- `rulesets/` - the committed ruleset artifacts: `rules-v0.synthetic.1` (v1) and `rules-v1.public` (v2)
- `tests/` - pytest suite (schema, injection corpus, gates, rulesets, golden cases, v2 engine, combiner, CLI)

## Engine v2 (triage)

The v2 engine accepts envelope v2 and runs Stage A, the deterministic gates of `rules-v1.public`.
With `--sku triage.v2` it also asks the advisory Stage B model judge from `../triage-model`. The
final decision is the minimum of the two stages, so the model can never raise a decision past
Stage A. See `../docs/engine/overview.md` and `../SCANNER_SPEC.md` (v2 section).

```
python -m aps_scanner < ../demo/envelope.json                  # v2 envelope: Stage A only
python -m aps_scanner --sku triage.v2 < ../demo/envelope.json  # Stage A + Stage B
```

The demo envelope lands on `needs_evidence` with the taxonomy hit
`unverified-deployment-evidence`, because its deployment reference is not a well-formed address or
hash. A v1 envelope sent without arguments still gets the v1 report, unchanged.

```python
from aps_scanner.envelope_v2 import FindingEnvelopeV2
from aps_scanner.engine_v2 import triage

report = triage(FindingEnvelopeV2(title="...", hypothesis="..."))
print(report.to_public_json()["decision"])
```

Runtime is stdlib-only. No network, no side effects, fully deterministic:
the same envelope always produces the same report and the same content hash.

## Running the tests

```
cd scanner
python -m pytest -q
```

## Usage

```python
from aps_scanner import FindingEnvelope, scan

envelope = FindingEnvelope(
    title="Fee-rounding drain in the settlement path",
    claimed_impact="An attacker drains value from the buffer (fund loss).",
    scope_statement="contracts/settlement/*.rs in the scope tree",
    attacker_capability="Any external caller; no special role needed.",
    preconditions="The path is permissionless and the buffer is warm.",
    deployed_state_evidence="Vault live at 0x1a2b3c4d5e6f7089 at block 19387002.",
    public_rules_snapshot="https://archive.example.org/rules/2026-10-20",
    reproduction_summary="1. Call settle() with amount 1. 2. Observe the drop.",
)
report = scan(envelope)
print(report.decision)  # supported / needs_evidence / held / rejected
print(report.confidence_band)  # high / medium / low
print(report.to_public_json_string())  # the public response body
```

Invalid envelopes raise `ScanInputError` carrying the list of validation
errors; the API layer maps that to a 400 response.

## Decision matrix

| decision | condition |
|---|---|
| `rejected` | any gate failed |
| `held` | no failures and a known-issue declaration is present |
| `needs_evidence` | no failures or declaration and some gate needs evidence |
| `supported` | all nine gates passed |

Confidence bands: `high` when supported with both deployed-state evidence and
a public rules snapshot; `medium` when supported with fewer or when the
decision is `needs_evidence`; `low` for `rejected` and `held`.

The report carries the ruleset version, the ruleset payload hash (the
sha256 of the compiled gates artifact, produced by aps-knowledge-compiler
in ../knowledge-compiler), and a content hash: the sha256 of the canonical
JSON of the normalized envelope, so identical submissions are bit-stable
across replays. A ruleset referencing an unknown evaluator or carrying a
tampered hash fails closed: the scan refuses to run rather than passing.
