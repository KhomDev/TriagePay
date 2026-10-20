"""Schema boundary and type validation tests."""

from aps_scanner import FindingEnvelope
from aps_scanner.schema import EVIDENCE_REF_MAX_LENGTH, EVIDENCE_REFS_MAX_COUNT


def test_title_boundary_accepts_200():
    assert FindingEnvelope(title="a" * 200).validate() == []


def test_title_boundary_rejects_201():
    errors = FindingEnvelope(title="a" * 201).validate()
    assert len(errors) == 1
    assert "title" in errors[0]
    assert "200" in errors[0]


def test_long_text_fields_accept_their_limits():
    envelope = FindingEnvelope(
        claimed_impact="i" * 4000,
        mechanism="m" * 4000,
        attacker_capability="a" * 4000,
        preconditions="p" * 4000,
        scope_statement="s" * 4000,
        deployed_state_evidence="d" * 4000,
        public_rules_snapshot="u" * 4000,
        reproduction_summary="r" * 4000,
    )
    assert envelope.validate() == []


def test_long_text_fields_reject_over_their_limits():
    envelope = FindingEnvelope(
        claimed_impact="i" * 4001,
        component_class="c" * 101,
    )
    errors = envelope.validate()
    assert len(errors) == 2
    joined = "\n".join(errors)
    assert "claimed_impact" in joined
    assert "component_class" in joined


def test_optional_field_limits():
    envelope = FindingEnvelope(
        known_issue_declaration="k" * 2000,
        code_excerpt="x" * 4000,
    )
    assert envelope.validate() == []
    envelope = FindingEnvelope(
        known_issue_declaration="k" * 2001,
        code_excerpt="x" * 4001,
    )
    errors = envelope.validate()
    assert len(errors) == 2


def test_optional_fields_default_to_none_and_validate():
    envelope = FindingEnvelope()
    assert envelope.known_issue_declaration is None
    assert envelope.code_excerpt is None
    assert envelope.validate() == []


def test_evidence_refs_accepts_sixteen():
    envelope = FindingEnvelope(
        evidence_refs=[f"ref-{i}" for i in range(EVIDENCE_REFS_MAX_COUNT)]
    )
    assert envelope.validate() == []


def test_evidence_refs_rejects_seventeen():
    envelope = FindingEnvelope(
        evidence_refs=[f"ref-{i}" for i in range(EVIDENCE_REFS_MAX_COUNT + 1)]
    )
    errors = envelope.validate()
    assert len(errors) == 1
    assert "evidence_refs" in errors[0]


def test_evidence_ref_length_boundaries():
    assert FindingEnvelope(evidence_refs=["r" * EVIDENCE_REF_MAX_LENGTH]).validate() == []
    errors = FindingEnvelope(evidence_refs=["r" * (EVIDENCE_REF_MAX_LENGTH + 1)]).validate()
    assert len(errors) == 1
    assert "evidence_refs[0]" in errors[0]


def test_wrong_field_types_rejected():
    envelope = FindingEnvelope(title=123, mechanism=None, evidence_refs="not-a-list")
    errors = envelope.validate()
    joined = "\n".join(errors)
    assert len(errors) == 3
    assert "title" in joined
    assert "mechanism" in joined
    assert "evidence_refs" in joined


def test_wrong_type_inside_evidence_refs():
    errors = FindingEnvelope(evidence_refs=["ok", 42]).validate()
    assert len(errors) == 1
    assert "evidence_refs[1]" in errors[0]


def test_valid_envelope_has_no_errors():
    envelope = FindingEnvelope(
        title="Fee-rounding drain",
        claimed_impact="fund loss",
        evidence_refs=["logs/poc.txt"],
    )
    assert envelope.validate() == []
