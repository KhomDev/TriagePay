"""FindingEnvelope schema with strict limits and total validation.

validate() returns a list of human-readable error strings and never raises.
"""

from dataclasses import dataclass, field

# Inclusive maximum length in characters for every str field.
FIELD_MAX_LENGTHS = {
    "title": 200,
    "claimed_impact": 4000,
    "mechanism": 4000,
    "attacker_capability": 4000,
    "preconditions": 4000,
    "component_class": 100,
    "scope_statement": 4000,
    "deployed_state_evidence": 4000,
    "public_rules_snapshot": 4000,
    "known_issue_declaration": 2000,
    "reproduction_summary": 4000,
    "code_excerpt": 4000,
}

# Fields that may be None instead of a str.
OPTIONAL_FIELDS = frozenset({"known_issue_declaration", "code_excerpt"})

EVIDENCE_REFS_MAX_COUNT = 16
EVIDENCE_REF_MAX_LENGTH = 300


@dataclass
class FindingEnvelope:
    """Hostile-input container for exactly one paid scan of a finding."""

    title: str = ""
    claimed_impact: str = ""
    mechanism: str = ""
    attacker_capability: str = ""
    preconditions: str = ""
    component_class: str = ""
    scope_statement: str = ""
    deployed_state_evidence: str = ""
    public_rules_snapshot: str = ""
    known_issue_declaration: str | None = None
    reproduction_summary: str = ""
    evidence_refs: list[str] = field(default_factory=list)
    code_excerpt: str | None = None

    def validate(self) -> list[str]:
        """Return a list of error strings; an empty list means the envelope is valid."""
        errors: list[str] = []
        for name, max_length in FIELD_MAX_LENGTHS.items():
            value = getattr(self, name)
            if value is None:
                if name not in OPTIONAL_FIELDS:
                    errors.append(f"{name} must be a string, not None")
                continue
            if not isinstance(value, str):
                errors.append(f"{name} must be a string")
                continue
            if len(value) > max_length:
                errors.append(f"{name} length {len(value)} exceeds the maximum of {max_length}")
        refs = self.evidence_refs
        if not isinstance(refs, list):
            errors.append("evidence_refs must be a list of strings")
        else:
            if len(refs) > EVIDENCE_REFS_MAX_COUNT:
                errors.append(
                    f"evidence_refs has {len(refs)} entries; at most "
                    f"{EVIDENCE_REFS_MAX_COUNT} are allowed"
                )
            for index, ref in enumerate(refs):
                if not isinstance(ref, str):
                    errors.append(f"evidence_refs[{index}] must be a string")
                elif len(ref) > EVIDENCE_REF_MAX_LENGTH:
                    errors.append(
                        f"evidence_refs[{index}] length {len(ref)} exceeds the "
                        f"maximum of {EVIDENCE_REF_MAX_LENGTH}"
                    )
        return errors
