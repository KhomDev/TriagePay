"""aps-scanner: deterministic scan engine for paid security-finding triage.

A caller pays one unit per scan of a submitted finding. The engine treats
every input field as hostile data, runs nine deterministic gates, and returns
a decision. It never interprets instructions embedded in the finding (they
are flagged, and the flag is advisory only) and never echoes the envelope.
"""

from .engine import (
    CONFIDENCE_HIGH,
    CONFIDENCE_LOW,
    CONFIDENCE_MEDIUM,
    DECISION_HELD,
    DECISION_NEEDS_EVIDENCE,
    DECISION_REJECTED,
    DECISION_SUPPORTED,
    DEFAULT_RULESET_VERSION,
    ScanInputError,
    scan,
)
from .gates import GATES, GateResult, GateStatus, run_gates
from .normalize import NormalizedEnvelope, normalize_envelope
from .report import ScanReport, compute_content_hash
from .ruleset import RulesetError, load_ruleset
from .ruleset import payload_hash as ruleset_payload_hash
from .schema import FindingEnvelope

__version__ = "0.1.0"

__all__ = [
    "FindingEnvelope",
    "NormalizedEnvelope",
    "normalize_envelope",
    "GateStatus",
    "GateResult",
    "GATES",
    "run_gates",
    "scan",
    "ScanInputError",
    "ScanReport",
    "compute_content_hash",
    "RulesetError",
    "load_ruleset",
    "ruleset_payload_hash",
    "DEFAULT_RULESET_VERSION",
    "DECISION_SUPPORTED",
    "DECISION_NEEDS_EVIDENCE",
    "DECISION_HELD",
    "DECISION_REJECTED",
    "CONFIDENCE_HIGH",
    "CONFIDENCE_MEDIUM",
    "CONFIDENCE_LOW",
]
