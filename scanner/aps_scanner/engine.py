"""Pipeline: schema validation, normalization, gates, decision matrix."""

from .gates import GateStatus, run_gates
from .normalize import normalize_envelope
from .report import ScanReport, compute_content_hash
from .ruleset import default_ruleset
from .ruleset import payload_hash as ruleset_payload_hash
from .schema import FindingEnvelope

DEFAULT_RULESET_VERSION = "rules-v0.synthetic.1"

DECISION_SUPPORTED = "supported"
DECISION_NEEDS_EVIDENCE = "needs_evidence"
DECISION_HELD = "held"
DECISION_REJECTED = "rejected"

CONFIDENCE_HIGH = "high"
CONFIDENCE_MEDIUM = "medium"
CONFIDENCE_LOW = "low"


class ScanInputError(ValueError):
    """Raised when the envelope fails schema validation.

    Carries the list of validation errors; the API layer maps it to a 400.
    """

    def __init__(self, errors):
        super().__init__("; ".join(errors) if errors else "invalid envelope")
        self.errors = list(errors)


def scan(
    envelope: FindingEnvelope,
    ruleset_version: str = None,
    ruleset: dict = None,
) -> ScanReport:
    """Scan one finding envelope deterministically. Same input, same report.

    The gates execute from the compiled ruleset (the bundled default
    unless one is passed); the report stamps the ruleset id and the
    payload hash so every decision is reproducible against the exact
    ruleset that produced it.
    """
    errors = envelope.validate()
    if errors:
        raise ScanInputError(errors)

    if ruleset is None:
        ruleset = default_ruleset()
    effective_version = ruleset_version or ruleset.get("ruleset_id", DEFAULT_RULESET_VERSION)

    normalized = normalize_envelope(envelope)
    gate_results = run_gates(normalized, ruleset)

    failed_gates = [result.gate for result in gate_results if result.status is GateStatus.FAIL]
    missing_evidence = [
        result.gate for result in gate_results if result.status is GateStatus.NEEDS_EVIDENCE
    ]

    if failed_gates:
        decision = DECISION_REJECTED
    elif normalized.known_issue_declaration:
        decision = DECISION_HELD
    elif missing_evidence:
        decision = DECISION_NEEDS_EVIDENCE
    else:
        decision = DECISION_SUPPORTED

    if (
        decision == DECISION_SUPPORTED
        and normalized.deployed_state_evidence
        and normalized.public_rules_snapshot
    ):
        confidence_band = CONFIDENCE_HIGH
    elif decision in (DECISION_SUPPORTED, DECISION_NEEDS_EVIDENCE):
        confidence_band = CONFIDENCE_MEDIUM
    else:
        confidence_band = CONFIDENCE_LOW

    return ScanReport(
        decision=decision,
        confidence_band=confidence_band,
        missing_evidence=missing_evidence,
        failed_gates=failed_gates,
        gate_results=gate_results,
        ruleset_version=effective_version,
        ruleset_hash=ruleset_payload_hash(ruleset),
        content_hash=compute_content_hash(normalized),
        contains_embedded_instructions=normalized.contains_embedded_instructions,
    )
