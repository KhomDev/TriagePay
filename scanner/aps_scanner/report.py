"""ScanReport and canonical content hashing for the public response."""

import hashlib
import json
from dataclasses import asdict, dataclass, field

from .normalize import NormalizedEnvelope


def compute_content_hash(normalized_envelope: NormalizedEnvelope) -> str:
    """sha256 hex of the canonical JSON of the normalized envelope dict."""
    payload = asdict(normalized_envelope)
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


@dataclass
class ScanReport:
    """Public outcome of one scan. Never echoes the raw envelope."""

    decision: str
    confidence_band: str
    missing_evidence: list = field(default_factory=list)
    failed_gates: list = field(default_factory=list)
    gate_results: list = field(default_factory=list)
    ruleset_version: str = ""
    ruleset_hash: str = ""
    content_hash: str = ""
    contains_embedded_instructions: bool = False

    def to_public_json(self) -> dict:
        """Return exactly the nine public report fields."""
        return {
            "decision": self.decision,
            "confidence_band": self.confidence_band,
            "missing_evidence": list(self.missing_evidence),
            "failed_gates": list(self.failed_gates),
            "gate_results": [
                {
                    "gate": result.gate,
                    "status": result.status.value,
                    "reason": result.reason,
                }
                for result in self.gate_results
            ],
            "ruleset_version": self.ruleset_version,
            "ruleset_hash": self.ruleset_hash,
            "content_hash": self.content_hash,
            "contains_embedded_instructions": self.contains_embedded_instructions,
        }

    def to_public_json_string(self) -> str:
        return json.dumps(self.to_public_json(), sort_keys=True, ensure_ascii=True)
