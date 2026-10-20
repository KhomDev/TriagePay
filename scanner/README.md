# aps-scanner

Deterministic scan engine for paid security-finding triage. A caller pays one
unit per scan of a submitted security finding; this package decides whether
that finding is worth a human look.

The engine treats every input field as hostile data. It never interprets
instructions embedded in a finding: such text is flagged, and the flag is
advisory only and never changes a gate result or the decision. The engine
never echoes the submitted envelope back.

## Layout

- `pyproject.toml` - package metadata and pytest configuration
- `aps_scanner/schema.py` - `FindingEnvelope` dataclass and validation
- `aps_scanner/normalize.py` - normalization and injection flagging
- `aps_scanner/gates.py` - the nine deterministic gates
- `aps_scanner/engine.py` - pipeline and decision matrix
- `aps_scanner/report.py` - `ScanReport` and public JSON serialization
- `tests/` - pytest suite (schema, injection corpus, gates, golden cases)

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
print(report.decision)                  # supported / needs_evidence / held / rejected
print(report.confidence_band)           # high / medium / low
print(report.to_public_json_string())   # the public response body
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

The report carries the ruleset version (default `rules-v0.synthetic.1`) and a
content hash: the sha256 of the canonical JSON of the normalized envelope, so
identical submissions are bit-stable across replays.
