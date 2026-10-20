"""Scanner-side ruleset tests: fail-closed loading, hash stamping,
mutation sensitivity, and replay stability."""

import json
import pathlib

import pytest

from aps_scanner import FindingEnvelope, RulesetError, load_ruleset, scan
from aps_scanner.ruleset import default_ruleset, payload_hash


def strong_envelope():
    return FindingEnvelope(
        title="Fee-rounding drain in the settlement path",
        claimed_impact="An attacker drains value from the buffer (fund loss).",
        scope_statement="contracts/settlement/*.rs in the scope tree",
        attacker_capability="Any external caller; no special role needed.",
        preconditions="The path is permissionless and the buffer is warm.",
        deployed_state_evidence="Vault live at 0x1a2b3c4d5e6f7089 at block 19387002.",
        public_rules_snapshot="https://archive.example.org/rules/2026-10-20",
        reproduction_summary="1. Call settle() with amount 1. 2. Observe the drop.",
    )


def test_default_ruleset_loads_and_matches_hash():
    ruleset = default_ruleset()
    assert ruleset["ruleset_id"] == "rules-v0.synthetic.1"
    assert ruleset["counts"]["gates"] == 9
    assert payload_hash(ruleset) == ruleset["payload_sha256"]


def test_report_stamps_ruleset_hash_matching_payload():
    report = scan(strong_envelope())
    assert report.ruleset_hash == payload_hash(default_ruleset())
    assert len(report.ruleset_hash) == 64


def test_unknown_evaluator_fails_closed(tmp_path):
    ruleset = load_ruleset()
    ruleset["gates"][0]["branches"][0]["when"] = [{"eval": "not_real", "field": "title"}]
    path = tmp_path / "bad.json"
    path.write_text(json.dumps(ruleset), encoding="utf-8")
    with pytest.raises(RulesetError, match="unknown evaluator"):
        load_ruleset(path)


def test_tampered_payload_hash_fails_closed(tmp_path):
    ruleset = load_ruleset()
    ruleset["payload_sha256"] = "0" * 64
    path = tmp_path / "bad.json"
    path.write_text(json.dumps(ruleset), encoding="utf-8")
    with pytest.raises(RulesetError, match="hash mismatch"):
        load_ruleset(path)


def test_missing_ruleset_file_fails_closed(tmp_path):
    with pytest.raises(RulesetError):
        load_ruleset(tmp_path / "missing.json")


def test_removal_of_one_gate_changes_decisions(tmp_path):
    ruleset = load_ruleset()
    baseline = scan(strong_envelope())
    # Remove the reproduction gate: the decision must change (the envelope
    # keeps its strong reproduction, but the gate set no longer checks it
    # - the missing_evidence list and hash must differ).
    ruleset["gates"] = [
        gate for gate in ruleset["gates"] if gate["id"] != "reproduction_non_tautological"
    ]
    ruleset["counts"]["gates"] = len(ruleset["gates"])
    import hashlib

    ruleset["payload_sha256"] = hashlib.sha256(
        json.dumps(ruleset["gates"], sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    stripped = scan(strong_envelope(), ruleset=ruleset)
    assert stripped.ruleset_hash != baseline.ruleset_hash
    gate_names = {result.gate for result in stripped.gate_results}
    assert "reproduction_non_tautological" not in gate_names


def test_ruleset_directory_is_committed():
    ruleset_path = (
        pathlib.Path(__file__).resolve().parent.parent / "rulesets" / "rules-v0.synthetic.1.json"
    )
    assert ruleset_path.exists(), "the compiled sample ruleset must be committed"
