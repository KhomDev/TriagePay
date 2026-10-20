"""The deterministic gates, executed from the compiled ruleset.

Each gate_* function keeps its historical name and signature (a
NormalizedEnvelope in, a GateResult out) so tests and callers are stable;
the logic itself is data-driven: the record comes from the bundled ruleset
artifact and is evaluated by aps_scanner.ruleset. Fail-closed: a missing
gate record or an unknown evaluator raises RulesetError and the scan
refuses to run.
"""

from dataclasses import dataclass
from enum import Enum

from .normalize import NormalizedEnvelope
from .ruleset import RulesetError, default_ruleset, evaluate_gate


class GateStatus(Enum):
    PASS = "Pass"
    FAIL = "Fail"
    NEEDS_EVIDENCE = "NeedsEvidence"


@dataclass
class GateResult:
    status: GateStatus
    reason: str
    gate: str = ""


# Stable public gate ids (the ruleset's gate ids; kept as module constants
# for callers that reference gates by name).
GATE_SCOPE_STATEMENT_PRESENT = "scope_statement_present"
GATE_ATTACKER_MODEL_PRESENT = "attacker_model_present"
GATE_PERMISSIONLESS_REACHABILITY = "permissionless_reachability"
GATE_IMPACT_SINK = "impact_sink"
GATE_REPRODUCTION_NON_TAUTOLOGICAL = "reproduction_non_tautological"
GATE_DEPLOYED_STATE_EVIDENCE = "deployed_state_evidence"
GATE_PUBLIC_RULES_SNAPSHOT = "public_rules_snapshot"
GATE_KNOWN_ISSUE_DECLARATION = "known_issue_declaration"
GATE_NO_LOSS_CONCESSION = "no_loss_concession"


_RECORDS = {}


def _record(gate_id: str):
    """The gate record for a stable public gate id (fail closed)."""
    if not _RECORDS:
        for record in default_ruleset()["gates"]:
            _RECORDS[record["id"]] = record
    if gate_id not in _RECORDS:
        raise RulesetError(f"gate '{gate_id}' is not in the loaded ruleset")
    return _RECORDS[gate_id]


def _run(gate_id: str, envelope: NormalizedEnvelope) -> GateResult:
    status, reason = evaluate_gate(_record(gate_id), envelope)
    return GateResult(GateStatus(status), reason, gate_id)


def gate_scope_statement_present(env: NormalizedEnvelope) -> GateResult:
    return _run("scope_statement_present", env)


def gate_attacker_model_present(env: NormalizedEnvelope) -> GateResult:
    return _run("attacker_model_present", env)


def gate_permissionless_reachability(env: NormalizedEnvelope) -> GateResult:
    return _run("permissionless_reachability", env)


def gate_impact_sink(env: NormalizedEnvelope) -> GateResult:
    return _run("impact_sink", env)


def gate_reproduction_non_tautological(env: NormalizedEnvelope) -> GateResult:
    return _run("reproduction_non_tautological", env)


def gate_deployed_state_evidence(env: NormalizedEnvelope) -> GateResult:
    return _run("deployed_state_evidence", env)


def gate_public_rules_snapshot(env: NormalizedEnvelope) -> GateResult:
    return _run("public_rules_snapshot", env)


def gate_known_issue_declaration(env: NormalizedEnvelope) -> GateResult:
    return _run("known_issue_declaration", env)


def gate_no_loss_concession(env: NormalizedEnvelope) -> GateResult:
    return _run("no_loss_concession", env)


GATES = (
    ("scope_statement_present", gate_scope_statement_present),
    ("attacker_model_present", gate_attacker_model_present),
    ("permissionless_reachability", gate_permissionless_reachability),
    ("impact_sink", gate_impact_sink),
    ("reproduction_non_tautological", gate_reproduction_non_tautological),
    ("deployed_state_evidence", gate_deployed_state_evidence),
    ("public_rules_snapshot", gate_public_rules_snapshot),
    ("known_issue_declaration", gate_known_issue_declaration),
    ("no_loss_concession", gate_no_loss_concession),
)


def run_gates(env: NormalizedEnvelope, ruleset: dict = None) -> list:
    """Run the ruleset's gates, in its order, against the envelope.

    Fully data-driven: the records come from the passed ruleset (the
    bundled default when absent); every result is built from the record
    itself, so a custom ruleset changes both execution and outcomes.
    """
    active = ruleset if ruleset is not None else default_ruleset()
    results = []
    for record in active["gates"]:
        status, reason = evaluate_gate(record, env)
        results.append(GateResult(GateStatus(status), reason, record["id"]))
    return results
