"""Deterministic combiner of Stage A (doctrine gates) and Stage B (model).

Decision order: rejected < held < needs_evidence < supported. The final
decision is the minimum of the two stages, so the model can lower a
decision or ask for evidence, but can never raise one past Stage A
(standing rule S9). Stage B maps its probability through two thresholds:

- p < t_low                -> rejected
- t_low <= p <= t_high     -> needs_evidence
- p > t_high               -> supported

A Stage B result that is not "ok" (fail-closed, unavailable) counts as
needs_evidence. When Stage B did not run, the final decision is Stage A's.

The default thresholds (0.35 / 0.65) are untuned until prompt 05 tunes
them on the public benchmark split.
"""

from dataclasses import dataclass

REJECTED = "rejected"
HELD = "held"
NEEDS_EVIDENCE = "needs_evidence"
SUPPORTED = "supported"

DECISION_ORDER = (REJECTED, HELD, NEEDS_EVIDENCE, SUPPORTED)
_RANK = {decision: rank for rank, decision in enumerate(DECISION_ORDER)}

STAGE_B_OK = "ok"
STAGE_B_FAIL_CLOSED = "fail_closed"
STAGE_B_UNAVAILABLE = "unavailable"
STAGE_B_NOT_RUN = "not_run"
STAGE_B_STATUSES = (STAGE_B_OK, STAGE_B_FAIL_CLOSED, STAGE_B_UNAVAILABLE, STAGE_B_NOT_RUN)

THRESHOLD_STATUS = "untuned until prompt 05"


@dataclass(frozen=True)
class Thresholds:
    t_low: float = 0.35
    t_high: float = 0.65
    status: str = THRESHOLD_STATUS

    def __post_init__(self):
        if not 0 <= self.t_low <= self.t_high <= 1:
            raise ValueError("thresholds must satisfy 0 <= t_low <= t_high <= 1")

    def public(self) -> dict:
        return {"t_low": self.t_low, "t_high": self.t_high, "status": self.status}


DEFAULT_THRESHOLDS = Thresholds()


def rank(decision: str) -> int:
    if decision not in _RANK:
        raise ValueError("unknown decision")
    return _RANK[decision]


def minimum(first: str, second: str) -> str:
    """The lower of two decisions in DECISION_ORDER."""
    return first if rank(first) <= rank(second) else second


def stage_b_decision(status: str, probability, thresholds: Thresholds = DEFAULT_THRESHOLDS):
    """Stage B's decision, or None when Stage B did not run."""
    if status == STAGE_B_NOT_RUN:
        return None
    if status != STAGE_B_OK or probability is None:
        return NEEDS_EVIDENCE
    if probability < thresholds.t_low:
        return REJECTED
    if probability > thresholds.t_high:
        return SUPPORTED
    return NEEDS_EVIDENCE


def combine(stage_a_decision: str, stage_b: str | None) -> str:
    """Final decision: Stage A alone, or the minimum of both stages."""
    rank(stage_a_decision)
    if stage_b is None:
        return stage_a_decision
    return minimum(stage_a_decision, stage_b)


def confidence_band(final: str, stage_a: str, stage_b_status: str, stage_b) -> str:
    """Confidence from agreement between the stages.

    - rejected or held: low;
    - needs_evidence: low when Stage B failed closed or was unavailable,
      else medium;
    - supported: high only when both stages ran and both say supported;
      medium for a Stage A-only supported verdict.
    """
    if final in (REJECTED, HELD):
        return "low"
    if final == NEEDS_EVIDENCE:
        if stage_b_status in (STAGE_B_FAIL_CLOSED, STAGE_B_UNAVAILABLE):
            return "low"
        return "medium"
    if stage_b_status == STAGE_B_OK and stage_a == SUPPORTED and stage_b == SUPPORTED:
        return "high"
    return "medium"


def benchmark_score(stage_b_status: str, probability, penalty_factor: float):
    """The benchmark score.

    - Stage B ok: probability times the Stage A penalty factor;
    - Stage B not run: the Stage A penalty factor alone (Stage A-only mode);
    - Stage B failed closed or unavailable: None (an unpredicted row).
    """
    if stage_b_status == STAGE_B_NOT_RUN:
        return round(penalty_factor, 6)
    if stage_b_status != STAGE_B_OK or probability is None:
        return None
    return round(probability * penalty_factor, 6)
