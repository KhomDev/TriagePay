"""Stage A: the deterministic doctrine gates of the v2 engine.

Stage A runs a schema 2 ruleset (rules-v1.public by default) over a
normalized v2 envelope. It can only refuse, hold, or ask for evidence:
its best outcome, "supported", means "no deterministic objection", and
the combiner never lets a later stage rise above it.

Decision: any Fail -> rejected; else any Held -> held; else any
NeedsEvidence -> needs_evidence; else supported. The taxonomy hits are
the public categories of every gate that did not pass. The penalty
factor is the product of the matched branches' penalties (fixed in the
ruleset, untuned until the benchmark evaluation).
"""

from dataclasses import dataclass, field

from .gates import GateStatus, run_gates
from .ruleset import default_v2_ruleset, payload_hash
from .taxonomy import ordered

DECISION_SUPPORTED = "supported"
DECISION_NEEDS_EVIDENCE = "needs_evidence"
DECISION_HELD = "held"
DECISION_REJECTED = "rejected"


@dataclass
class StageAResult:
    decision: str
    gate_results: list = field(default_factory=list)
    failed_gates: list = field(default_factory=list)
    held_gates: list = field(default_factory=list)
    missing_evidence: list = field(default_factory=list)
    taxonomy_hits: list = field(default_factory=list)
    penalty_factor: float = 1.0
    ruleset_version: str = ""
    ruleset_hash: str = ""

    def gate_passed(self, gate_id: str) -> bool:
        return any(r.gate == gate_id and r.status is GateStatus.PASS for r in self.gate_results)


def run_stage_a(normalized, ruleset=None) -> StageAResult:
    """Run the v2 gates over a normalized v2 envelope."""
    ruleset = default_v2_ruleset() if ruleset is None else ruleset
    results = run_gates(normalized, ruleset)

    def with_status(status):
        return [r.gate for r in results if r.status is status]

    failed = with_status(GateStatus.FAIL)
    held = with_status(GateStatus.HELD)
    missing = with_status(GateStatus.NEEDS_EVIDENCE)
    if failed:
        decision = DECISION_REJECTED
    elif held:
        decision = DECISION_HELD
    elif missing:
        decision = DECISION_NEEDS_EVIDENCE
    else:
        decision = DECISION_SUPPORTED

    penalty = 1.0
    for result in results:
        if result.status is not GateStatus.PASS:
            penalty *= result.penalty
    hits = ordered(r.category for r in results if r.status is not GateStatus.PASS)
    return StageAResult(
        decision=decision,
        gate_results=results,
        failed_gates=failed,
        held_gates=held,
        missing_evidence=missing,
        taxonomy_hits=hits,
        penalty_factor=round(penalty, 6),
        ruleset_version=ruleset["ruleset_id"],
        ruleset_hash=payload_hash(ruleset),
    )
