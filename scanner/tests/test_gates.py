"""Branch coverage for each of the nine gates."""

from aps_scanner import FindingEnvelope, normalize_envelope
from aps_scanner.gates import (
    GATE_PERMISSIONLESS_REACHABILITY,
    GateStatus,
    gate_attacker_model_present,
    gate_deployed_state_evidence,
    gate_impact_sink,
    gate_known_issue_declaration,
    gate_no_loss_concession,
    gate_permissionless_reachability,
    gate_public_rules_snapshot,
    gate_reproduction_non_tautological,
    gate_scope_statement_present,
    run_gates,
)


def norm(**overrides):
    return normalize_envelope(FindingEnvelope(**overrides))


def test_scope_statement_present():
    passed = gate_scope_statement_present(norm(scope_statement="contracts/vault"))
    assert passed.status is GateStatus.PASS
    assert passed.gate == "scope_statement_present"
    failed = gate_scope_statement_present(norm(scope_statement=""))
    assert failed.status is GateStatus.FAIL
    assert failed.gate == "scope_statement_present"


def test_attacker_model_present():
    passed = gate_attacker_model_present(norm(attacker_capability="any caller"))
    assert passed.status is GateStatus.PASS
    needs = gate_attacker_model_present(norm(attacker_capability=""))
    assert needs.status is GateStatus.NEEDS_EVIDENCE


def test_permissionless_reachability_fail_on_privileged():
    result = gate_permissionless_reachability(
        norm(
            attacker_capability="the contract owner",
            preconditions="the owner must first disable the rate limit",
        )
    )
    assert result.status is GateStatus.FAIL
    assert result.gate == GATE_PERMISSIONLESS_REACHABILITY


def test_permissionless_reachability_pass_with_escape():
    result = gate_permissionless_reachability(
        norm(
            attacker_capability="a privileged operator role today",
            preconditions="a permissionless alternative exists after the upgrade",
        )
    )
    assert result.status is GateStatus.PASS


def test_permissionless_reachability_needs_evidence_when_empty():
    result = gate_permissionless_reachability(
        norm(attacker_capability="", preconditions="none required")
    )
    assert result.status is GateStatus.NEEDS_EVIDENCE


def test_impact_sink_branches():
    passed = gate_impact_sink(norm(claimed_impact="drain of pooled funds"))
    assert passed.status is GateStatus.PASS
    failed = gate_impact_sink(norm(claimed_impact=""))
    assert failed.status is GateStatus.FAIL
    needs = gate_impact_sink(norm(claimed_impact="the interface renders stale labels"))
    assert needs.status is GateStatus.NEEDS_EVIDENCE


def test_reproduction_non_tautological_branches():
    passed = gate_reproduction_non_tautological(
        norm(reproduction_summary=("1. Call settle() with amount 1. 2. Observe the balance drop."))
    )
    assert passed.status is GateStatus.PASS
    failed = gate_reproduction_non_tautological(norm(reproduction_summary="call settle twice"))
    assert failed.status is GateStatus.FAIL
    needs = gate_reproduction_non_tautological(
        norm(
            reproduction_summary=(
                "the invariant breaks during the setup phase after ordinary usage"
            )
        )
    )
    assert needs.status is GateStatus.NEEDS_EVIDENCE


def test_deployed_state_evidence_branches():
    hex_pass = gate_deployed_state_evidence(
        norm(deployed_state_evidence="vault at 0xdeadbeefcafe01 is live")
    )
    assert hex_pass.status is GateStatus.PASS
    block_pass = gate_deployed_state_evidence(
        norm(deployed_state_evidence="confirmed at block 19387002")
    )
    assert block_pass.status is GateStatus.PASS
    tight_block_pass = gate_deployed_state_evidence(
        norm(deployed_state_evidence="seen at block19387002")
    )
    assert tight_block_pass.status is GateStatus.PASS
    empty = gate_deployed_state_evidence(norm(deployed_state_evidence=""))
    assert empty.status is GateStatus.NEEDS_EVIDENCE
    weak = gate_deployed_state_evidence(
        norm(deployed_state_evidence="the vault is live per the deployment log")
    )
    assert weak.status is GateStatus.NEEDS_EVIDENCE
    short_hex = gate_deployed_state_evidence(norm(deployed_state_evidence="seen at 0x1234 only"))
    assert short_hex.status is GateStatus.NEEDS_EVIDENCE


def test_public_rules_snapshot_branches():
    passed = gate_public_rules_snapshot(
        norm(public_rules_snapshot="https://archive.example.org/rules/1")
    )
    assert passed.status is GateStatus.PASS
    passed_http = gate_public_rules_snapshot(
        norm(public_rules_snapshot="http://archive.example.org/rules/1")
    )
    assert passed_http.status is GateStatus.PASS
    empty = gate_public_rules_snapshot(norm(public_rules_snapshot=""))
    assert empty.status is GateStatus.NEEDS_EVIDENCE
    not_a_link = gate_public_rules_snapshot(
        norm(public_rules_snapshot="see the attached rules pdf")
    )
    assert not_a_link.status is GateStatus.NEEDS_EVIDENCE


def test_known_issue_declaration_always_passes():
    absent = gate_known_issue_declaration(norm(known_issue_declaration=None))
    assert absent.status is GateStatus.PASS
    empty = gate_known_issue_declaration(norm(known_issue_declaration=""))
    assert empty.status is GateStatus.PASS
    present = gate_known_issue_declaration(
        norm(known_issue_declaration="possible duplicate of KI-14")
    )
    assert present.status is GateStatus.PASS


def test_no_loss_concession_branches():
    passed = gate_no_loss_concession(
        norm(
            claimed_impact="attacker drains pooled funds",
            reproduction_summary="1. Call the sink and observe the drain.",
        )
    )
    assert passed.status is GateStatus.PASS
    failed = gate_no_loss_concession(
        norm(
            claimed_impact="causes fund loss to stakers",
            reproduction_summary=("1. Replay the call. Note that no funds are lost in this flow."),
        )
    )
    assert failed.status is GateStatus.FAIL
    concession_without_impact = gate_no_loss_concession(
        norm(
            claimed_impact="the interface flickers during the check",
            reproduction_summary="replay and note that no fund loss occurs",
        )
    )
    assert concession_without_impact.status is GateStatus.PASS


def test_run_gates_returns_nine_results_in_spec_order():
    results = run_gates(norm())
    assert [result.gate for result in results] == [
        "scope_statement_present",
        "attacker_model_present",
        "permissionless_reachability",
        "impact_sink",
        "reproduction_non_tautological",
        "deployed_state_evidence",
        "public_rules_snapshot",
        "known_issue_declaration",
        "no_loss_concession",
    ]
