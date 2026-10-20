"""The nine deterministic gates.

Every gate takes a NormalizedEnvelope and returns a GateResult. Reasons are
short public strings: no private taxonomy, no host paths, no protocol names.
"""

import re
from dataclasses import dataclass
from enum import Enum

from .normalize import NormalizedEnvelope


class GateStatus(Enum):
    PASS = "Pass"
    FAIL = "Fail"
    NEEDS_EVIDENCE = "NeedsEvidence"


@dataclass
class GateResult:
    status: GateStatus
    reason: str
    gate: str = ""


GATE_SCOPE_STATEMENT_PRESENT = "scope_statement_present"
GATE_ATTACKER_MODEL_PRESENT = "attacker_model_present"
GATE_PERMISSIONLESS_REACHABILITY = "permissionless_reachability"
GATE_IMPACT_SINK = "impact_sink"
GATE_REPRODUCTION_NON_TAUTOLOGICAL = "reproduction_non_tautological"
GATE_DEPLOYED_STATE_EVIDENCE = "deployed_state_evidence"
GATE_PUBLIC_RULES_SNAPSHOT = "public_rules_snapshot"
GATE_KNOWN_ISSUE_DECLARATION = "known_issue_declaration"
GATE_NO_LOSS_CONCESSION = "no_loss_concession"

PRIVILEGED_TERMS = (
    "admin",
    "owner",
    "operator",
    "privileged",
    "multisig signer",
    "only owner",
)
# The escape hatch is the spec phrase "permissionless or unprivileged
# alternative", matched as the keywords "permissionless" / "unprivileged".
PERMISSIONLESS_TERMS = ("permissionless", "unprivileged")
IMPACT_TERMS = ("fund", "asset", "value", "loss", "theft", "drain", "mint", "freeze")
REPRODUCTION_TERMS = ("step", "call", "transaction", "reproduce", "1.", "2.")
CONCESSION_PHRASES = (
    "no funds are lost",
    "no fund loss",
    "no value is extracted",
)
CONCESSION_IMPACT_TERMS = ("fund", "loss", "theft", "drain")

_HEX_RUN = re.compile(r"0x[0-9a-f]{8,}", re.IGNORECASE)
_BLOCK_NUMBER = re.compile(r"\bblock\s*[0-9]")


def gate_scope_statement_present(env: NormalizedEnvelope) -> GateResult:
    if not env.scope_statement:
        return GateResult(
            GateStatus.FAIL,
            "scope statement is missing",
            GATE_SCOPE_STATEMENT_PRESENT,
        )
    return GateResult(
        GateStatus.PASS, "scope statement provided", GATE_SCOPE_STATEMENT_PRESENT
    )


def gate_attacker_model_present(env: NormalizedEnvelope) -> GateResult:
    if not env.attacker_capability:
        return GateResult(
            GateStatus.NEEDS_EVIDENCE,
            "attacker capability not provided",
            GATE_ATTACKER_MODEL_PRESENT,
        )
    return GateResult(
        GateStatus.PASS, "attacker capability provided", GATE_ATTACKER_MODEL_PRESENT
    )


def gate_permissionless_reachability(env: NormalizedEnvelope) -> GateResult:
    combined = (env.attacker_capability + env.preconditions).lower()
    escape = any(
        term in env.preconditions.lower() for term in PERMISSIONLESS_TERMS
    )
    if any(term in combined for term in PRIVILEGED_TERMS) and not escape:
        return GateResult(
            GateStatus.FAIL,
            "attacker path depends on a privileged role without a "
            "permissionless alternative",
            GATE_PERMISSIONLESS_REACHABILITY,
        )
    if not env.attacker_capability:
        return GateResult(
            GateStatus.NEEDS_EVIDENCE,
            "attacker capability not provided",
            GATE_PERMISSIONLESS_REACHABILITY,
        )
    return GateResult(
        GateStatus.PASS,
        "attacker path is reachable without privileged roles",
        GATE_PERMISSIONLESS_REACHABILITY,
    )


def gate_impact_sink(env: NormalizedEnvelope) -> GateResult:
    impact = env.claimed_impact
    if not impact:
        return GateResult(
            GateStatus.FAIL, "claimed impact is empty", GATE_IMPACT_SINK
        )
    lowered = impact.lower()
    if not any(term in lowered for term in IMPACT_TERMS):
        return GateResult(
            GateStatus.NEEDS_EVIDENCE,
            "claimed impact does not name a value impact",
            GATE_IMPACT_SINK,
        )
    return GateResult(
        GateStatus.PASS, "claimed impact names a value impact", GATE_IMPACT_SINK
    )


def gate_reproduction_non_tautological(env: NormalizedEnvelope) -> GateResult:
    reproduction = env.reproduction_summary
    if len(reproduction) < 40:
        return GateResult(
            GateStatus.FAIL,
            "reproduction summary is empty or too short",
            GATE_REPRODUCTION_NON_TAUTOLOGICAL,
        )
    lowered = reproduction.lower()
    if not any(term in lowered for term in REPRODUCTION_TERMS):
        return GateResult(
            GateStatus.NEEDS_EVIDENCE,
            "reproduction summary lacks concrete steps",
            GATE_REPRODUCTION_NON_TAUTOLOGICAL,
        )
    return GateResult(
        GateStatus.PASS,
        "reproduction summary describes concrete steps",
        GATE_REPRODUCTION_NON_TAUTOLOGICAL,
    )


def gate_deployed_state_evidence(env: NormalizedEnvelope) -> GateResult:
    evidence = env.deployed_state_evidence
    if not evidence:
        return GateResult(
            GateStatus.NEEDS_EVIDENCE,
            "deployed-state evidence is empty",
            GATE_DEPLOYED_STATE_EVIDENCE,
        )
    if _HEX_RUN.search(evidence) or _BLOCK_NUMBER.search(evidence):
        return GateResult(
            GateStatus.PASS,
            "deployed-state evidence cites on-chain data",
            GATE_DEPLOYED_STATE_EVIDENCE,
        )
    return GateResult(
        GateStatus.NEEDS_EVIDENCE,
        "deployed-state evidence lacks an on-chain reference",
        GATE_DEPLOYED_STATE_EVIDENCE,
    )


def gate_public_rules_snapshot(env: NormalizedEnvelope) -> GateResult:
    snapshot = env.public_rules_snapshot
    if not snapshot:
        return GateResult(
            GateStatus.NEEDS_EVIDENCE,
            "public rules snapshot is empty",
            GATE_PUBLIC_RULES_SNAPSHOT,
        )
    if snapshot.startswith("http://") or snapshot.startswith("https://"):
        return GateResult(
            GateStatus.PASS,
            "public rules snapshot is a web link",
            GATE_PUBLIC_RULES_SNAPSHOT,
        )
    return GateResult(
        GateStatus.NEEDS_EVIDENCE,
        "public rules snapshot is not a web link",
        GATE_PUBLIC_RULES_SNAPSHOT,
    )


def gate_known_issue_declaration(env: NormalizedEnvelope) -> GateResult:
    if env.known_issue_declaration:
        return GateResult(
            GateStatus.PASS,
            "known-issue declaration provided; handled by the decision matrix",
            GATE_KNOWN_ISSUE_DECLARATION,
        )
    return GateResult(
        GateStatus.PASS,
        "no known-issue declaration",
        GATE_KNOWN_ISSUE_DECLARATION,
    )


def gate_no_loss_concession(env: NormalizedEnvelope) -> GateResult:
    combined = (env.reproduction_summary + env.claimed_impact).lower()
    conceded = any(phrase in combined for phrase in CONCESSION_PHRASES)
    impact_claimed = any(
        term in env.claimed_impact.lower() for term in CONCESSION_IMPACT_TERMS
    )
    if conceded and impact_claimed:
        return GateResult(
            GateStatus.FAIL,
            "a no-loss concession contradicts the claimed impact",
            GATE_NO_LOSS_CONCESSION,
        )
    return GateResult(
        GateStatus.PASS,
        "no contradicting no-loss concession",
        GATE_NO_LOSS_CONCESSION,
    )


GATES = (
    (GATE_SCOPE_STATEMENT_PRESENT, gate_scope_statement_present),
    (GATE_ATTACKER_MODEL_PRESENT, gate_attacker_model_present),
    (GATE_PERMISSIONLESS_REACHABILITY, gate_permissionless_reachability),
    (GATE_IMPACT_SINK, gate_impact_sink),
    (GATE_REPRODUCTION_NON_TAUTOLOGICAL, gate_reproduction_non_tautological),
    (GATE_DEPLOYED_STATE_EVIDENCE, gate_deployed_state_evidence),
    (GATE_PUBLIC_RULES_SNAPSHOT, gate_public_rules_snapshot),
    (GATE_KNOWN_ISSUE_DECLARATION, gate_known_issue_declaration),
    (GATE_NO_LOSS_CONCESSION, gate_no_loss_concession),
)


def run_gates(env: NormalizedEnvelope) -> list:
    """Run all nine gates in spec order and return their results in order."""
    return [gate(env) for _, gate in GATES]
