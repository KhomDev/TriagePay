"""Golden decision cases, differential renaming, privacy, and replay."""

import importlib.util
import pathlib
import re

import pytest

from aps_scanner import FindingEnvelope, ScanInputError, scan

PROTOCOL_TOKEN = "acme-vault"
RENAMED_TOKEN = "zephyr-bridge"


def golden_envelope(protocol: str = PROTOCOL_TOKEN) -> FindingEnvelope:
    return FindingEnvelope(
        title="Fee-rounding drain in the settlement path of " + protocol,
        claimed_impact=(
            "An attacker drains accumulated value from the "
            + protocol
            + " settlement buffer, causing direct fund loss to stakers."
        ),
        mechanism=(
            "Rounding in the "
            + protocol
            + " settlement step lets a caller skim dust per call and drain "
            "the buffer."
        ),
        attacker_capability=(
            "Any external caller with a funded " + protocol + " account; no special role is needed."
        ),
        preconditions=("The " + protocol + " path is permissionless and the buffer is warm."),
        component_class="settlement",
        scope_statement=protocol + "/contracts/settlement/*.rs in the scope tree",
        deployed_state_evidence=(
            protocol + " settlement vault live at 0x1a2b3c4d5e6f7089 confirmed on block 19387002."
        ),
        public_rules_snapshot="https://archive.example.org/rules/2026-10-20",
        known_issue_declaration=None,
        reproduction_summary=(
            "1. Call settle() with amount 1. 2. Repeat the call 100 "
            "times. 3. Observe the buffer balance drop by 100 units."
        ),
        evidence_refs=[protocol + "/logs/poc-run-7.txt"],
        code_excerpt=None,
    )


def _status_map(report):
    return {result.gate: result.status.value for result in report.gate_results}


def test_supported_with_all_fields_strong():
    report = scan(golden_envelope())
    assert report.decision == "supported"
    assert report.confidence_band == "high"
    assert report.missing_evidence == []
    assert report.failed_gates == []
    assert len(report.gate_results) == 9
    assert set(_status_map(report).values()) == {"Pass"}
    assert report.contains_embedded_instructions is False


def test_needs_evidence_when_deployed_state_missing():
    envelope = golden_envelope()
    envelope.deployed_state_evidence = ""
    report = scan(envelope)
    assert report.decision == "needs_evidence"
    assert report.confidence_band == "medium"
    assert report.missing_evidence == ["deployed_state_evidence"]
    assert report.failed_gates == []


def test_rejected_privileged_attacker_without_permissionless_path():
    envelope = golden_envelope()
    envelope.attacker_capability = "the contract owner"
    envelope.preconditions = "the owner must first disable the rate limit"
    report = scan(envelope)
    assert report.decision == "rejected"
    assert report.confidence_band == "low"
    assert report.failed_gates == ["permissionless_reachability"]
    assert report.missing_evidence == []


def test_held_when_known_issue_declaration_set():
    envelope = golden_envelope()
    envelope.known_issue_declaration = "possible duplicate of known issue KI-14"
    report = scan(envelope)
    assert report.decision == "held"
    assert report.confidence_band == "low"
    assert report.failed_gates == []
    assert report.missing_evidence == []


def test_rejected_no_loss_concession():
    envelope = golden_envelope()
    envelope.reproduction_summary = (
        "1. Call settle() with amount 1. 2. Repeat the call. "
        "Note that no funds are lost in this flow."
    )
    report = scan(envelope)
    assert report.decision == "rejected"
    assert report.confidence_band == "low"
    assert report.failed_gates == ["no_loss_concession"]


def test_rejected_reproduction_under_40_chars():
    envelope = golden_envelope()
    envelope.reproduction_summary = "call settle twice"
    report = scan(envelope)
    assert report.decision == "rejected"
    assert report.failed_gates == ["reproduction_non_tautological"]


def test_fail_dominates_needs_evidence():
    envelope = golden_envelope()
    envelope.attacker_capability = "the contract owner"
    envelope.preconditions = "the owner must first disable the rate limit"
    envelope.deployed_state_evidence = ""
    report = scan(envelope)
    assert report.decision == "rejected"
    assert report.failed_gates == ["permissionless_reachability"]
    assert report.missing_evidence == ["deployed_state_evidence"]


def test_held_dominates_needs_evidence():
    envelope = golden_envelope()
    envelope.known_issue_declaration = "flagged against the known-issues list"
    envelope.deployed_state_evidence = ""
    report = scan(envelope)
    assert report.decision == "held"
    assert report.missing_evidence == ["deployed_state_evidence"]


def test_schema_errors_raise_scan_input_error():
    envelope = golden_envelope()
    envelope.title = "x" * 201
    envelope.component_class = 7
    with pytest.raises(ScanInputError) as excinfo:
        scan(envelope)
    assert len(excinfo.value.errors) == 2
    assert "title" in excinfo.value.errors[0]


def test_renaming_protocol_keeps_decision_and_gate_statuses():
    first = scan(golden_envelope(PROTOCOL_TOKEN))
    second = scan(golden_envelope(RENAMED_TOKEN))
    assert first.decision == second.decision == "supported"
    assert first.confidence_band == second.confidence_band == "high"
    assert _status_map(first) == _status_map(second)
    assert first.content_hash != second.content_hash


def test_public_json_exposes_exactly_the_spec_fields():
    payload = scan(golden_envelope()).to_public_json()
    assert set(payload) == {
        "decision",
        "confidence_band",
        "missing_evidence",
        "failed_gates",
        "gate_results",
        "ruleset_version",
        "ruleset_hash",
        "content_hash",
        "contains_embedded_instructions",
    }
    assert payload["ruleset_version"] == "rules-v0.synthetic.1"
    # The ruleset hash pins the exact compiled ruleset behind the report.
    assert len(payload["ruleset_hash"]) == 64
    assert payload["gate_results"][0]["gate"] == "scope_statement_present"
    assert payload["gate_results"][0]["status"] == "Pass"
    assert set(payload["gate_results"][0]) == {"gate", "status", "reason"}


# Private terms are checked by digest, never written here: the shared
# denylist mechanism lives in the knowledge compiler (stdlib-only module,
# loaded by path so the scanner package keeps zero dependencies). It loads
# the published synthetic stand-in digests; real terms are checked only
# when APS_PRIVATE_DENYLIST names the operator's off-repo digest file.
LEAKAGE_PATH = (
    pathlib.Path(__file__).resolve().parents[2]
    / "knowledge-compiler"
    / "knowledge_compiler"
    / "leakage.py"
)
# Host-path prefixes are structural and reveal nothing, so they stay literal.
FORBIDDEN_HOST_PATHS = ["c:\\", "/users/", "/home/"]


def _load_leakage():
    spec = importlib.util.spec_from_file_location("aps_leakage_for_tests", LEAKAGE_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_public_json_leaks_nothing_private():
    leakage = _load_leakage()
    digests = leakage.load_denylist()
    assert digests, "the published denylist must not be empty"
    text = scan(golden_envelope()).to_public_json_string()
    assert leakage.count_denylisted(text, digests) == 0
    lowered = text.lower()
    for token in FORBIDDEN_HOST_PATHS:
        assert token not in lowered


def test_privacy_check_is_not_vacuous():
    # Positive control with a synthetic stand-in: the same check that
    # passes above fires when a denylisted token is present.
    leakage = _load_leakage()
    stand_in = frozenset({leakage.digest_for_term("zz-private-alpha")})
    text = scan(golden_envelope()).to_public_json_string()
    assert leakage.count_denylisted(text, stand_in) == 0
    assert leakage.count_denylisted(text + " ZzPrivateAlpha", stand_in) == 1


def test_replay_is_bit_stable():
    envelope = golden_envelope()
    first = scan(envelope)
    second = scan(envelope)
    assert first.content_hash == second.content_hash
    assert first.to_public_json_string() == second.to_public_json_string()
    assert re.fullmatch("[0-9a-f]{64}", first.content_hash)
