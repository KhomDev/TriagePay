"""Combiner: exhaustive 4x4 table, thresholds, and the never-upgrade property."""

import itertools

import pytest
from v2_fixtures import STAGE_A_FIXTURES, judge_returning, strong, verdict

from aps_scanner import combiner
from aps_scanner.combiner import DECISION_ORDER, Thresholds
from aps_scanner.engine_v2 import triage

# Expected final decision for every (Stage A, Stage B) pair, written out by
# hand so the table itself is the specification.
TABLE = {
    ("rejected", "rejected"): "rejected",
    ("rejected", "held"): "rejected",
    ("rejected", "needs_evidence"): "rejected",
    ("rejected", "supported"): "rejected",
    ("held", "rejected"): "rejected",
    ("held", "held"): "held",
    ("held", "needs_evidence"): "held",
    ("held", "supported"): "held",
    ("needs_evidence", "rejected"): "rejected",
    ("needs_evidence", "held"): "held",
    ("needs_evidence", "needs_evidence"): "needs_evidence",
    ("needs_evidence", "supported"): "needs_evidence",
    ("supported", "rejected"): "rejected",
    ("supported", "held"): "held",
    ("supported", "needs_evidence"): "needs_evidence",
    ("supported", "supported"): "supported",
}


def test_table_is_exhaustive():
    assert set(TABLE) == set(itertools.product(DECISION_ORDER, DECISION_ORDER))


@pytest.mark.parametrize("pair", sorted(TABLE), ids=lambda p: f"{p[0]}+{p[1]}")
def test_combine_table(pair):
    stage_a, stage_b = pair
    final = combiner.combine(stage_a, stage_b)
    assert final == TABLE[pair]
    # The model can never upgrade a verdict past Stage A.
    assert combiner.rank(final) <= combiner.rank(stage_a)


@pytest.mark.parametrize("stage_a", DECISION_ORDER)
def test_stage_b_not_run_keeps_stage_a(stage_a):
    assert combiner.combine(stage_a, None) == stage_a


def test_unknown_decisions_raise():
    with pytest.raises(ValueError):
        combiner.combine("approved", None)
    with pytest.raises(ValueError):
        combiner.combine("supported", "approved")


@pytest.mark.parametrize(
    "probability, expected",
    [
        (0.0, "rejected"),
        (0.3499, "rejected"),
        (0.35, "needs_evidence"),
        (0.5, "needs_evidence"),
        (0.65, "needs_evidence"),
        (0.6501, "supported"),
        (1.0, "supported"),
    ],
)
def test_default_threshold_mapping(probability, expected):
    assert combiner.stage_b_decision("ok", probability) == expected


def test_thresholds_are_configurable_and_labelled_untuned():
    custom = Thresholds(t_low=0.2, t_high=0.9)
    assert combiner.stage_b_decision("ok", 0.5, custom) == "needs_evidence"
    assert combiner.stage_b_decision("ok", 0.95, custom) == "supported"
    assert combiner.DEFAULT_THRESHOLDS.public() == {
        "t_low": 0.35,
        "t_high": 0.65,
        "status": "untuned until prompt 05",
    }
    with pytest.raises(ValueError):
        Thresholds(t_low=0.8, t_high=0.2)


@pytest.mark.parametrize("status", ["fail_closed", "unavailable"])
def test_failed_stage_b_counts_as_needs_evidence(status):
    assert combiner.stage_b_decision(status, None) == "needs_evidence"
    assert combiner.stage_b_decision(status, 0.99) == "needs_evidence"


def test_stage_b_not_run_has_no_decision():
    assert combiner.stage_b_decision("not_run", None) is None


# The end-to-end 4x4: each Stage A decision (real gates over synthetic
# envelopes) against a judge scripted to produce each Stage B decision.
STAGE_B_PROBABILITIES = {"rejected": 0.1, "needs_evidence": 0.5, "supported": 0.99}


@pytest.mark.parametrize(
    "stage_a, stage_b",
    list(itertools.product(sorted(STAGE_A_FIXTURES), sorted(STAGE_B_PROBABILITIES))),
)
def test_pipeline_never_upgrades_past_stage_a(stage_a, stage_b):
    envelope = strong(**STAGE_A_FIXTURES[stage_a])
    judge = judge_returning(verdict(STAGE_B_PROBABILITIES[stage_b]))
    report = triage(envelope, judge=judge)
    assert report.stage_a.decision == stage_a
    assert report.stage_b_decision == stage_b
    assert report.decision == TABLE[(stage_a, stage_b)]
    assert combiner.rank(report.decision) <= combiner.rank(stage_a)


def test_confidence_bands():
    assert combiner.confidence_band("rejected", "rejected", "ok", "rejected") == "low"
    assert combiner.confidence_band("held", "held", "not_run", None) == "low"
    assert combiner.confidence_band("needs_evidence", "supported", "fail_closed", "needs_evidence") == "low"
    assert combiner.confidence_band("needs_evidence", "needs_evidence", "not_run", None) == "medium"
    assert combiner.confidence_band("supported", "supported", "not_run", None) == "medium"
    assert combiner.confidence_band("supported", "supported", "ok", "supported") == "high"


def test_benchmark_score():
    assert combiner.benchmark_score("not_run", None, 0.8) == 0.8
    assert combiner.benchmark_score("ok", 0.5, 0.8) == 0.4
    assert combiner.benchmark_score("fail_closed", None, 0.8) is None
