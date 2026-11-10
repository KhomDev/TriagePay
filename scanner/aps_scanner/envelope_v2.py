"""Finding envelope v2: a strict superset of the v1 FindingEnvelope.

v2 keeps every v1 field and limit, and adds:

- ``hypothesis``: free text, at most 12000 characters;
- ``source_ref``: optional ``{"repo_url": str, "commit": str}``;
- ``code_context``: at most 16000 characters;
- ``program_rules``: caller-supplied scope and exclusion text, at most 8000
  characters.

Mappings are deterministic and total:

- ``from_v1`` copies every v1 field and leaves the v2 fields at their
  defaults, so a v1 envelope always maps to the same v2 envelope;
- ``from_benchmark_row`` maps a ``{id, title, hypothesis}`` row by
  truncating the title and hypothesis to their limits; the row id is NOT
  part of the envelope, so the content hash depends on the finding only.

``load_payload`` is the single entry point the CLI uses: it decides the
envelope version from the payload, rejects unknown fields without echoing
their names, and returns a v1 or v2 envelope for schema validation.
"""

from dataclasses import dataclass

from .schema import FindingEnvelope

ENVELOPE_VERSION_FIELD = "envelope_version"

V1_FIELDS = frozenset(
    {
        "title",
        "claimed_impact",
        "mechanism",
        "attacker_capability",
        "preconditions",
        "component_class",
        "scope_statement",
        "deployed_state_evidence",
        "public_rules_snapshot",
        "known_issue_declaration",
        "reproduction_summary",
        "evidence_refs",
        "code_excerpt",
    }
)

V2_ONLY_FIELDS = frozenset({"hypothesis", "source_ref", "code_context", "program_rules"})

ALL_FIELDS = V1_FIELDS | V2_ONLY_FIELDS | {ENVELOPE_VERSION_FIELD}

# Inclusive maxima for the v2 text fields.
V2_FIELD_MAX_LENGTHS = {
    "hypothesis": 12000,
    "code_context": 16000,
    "program_rules": 8000,
}

SOURCE_REF_FIELDS = frozenset({"repo_url", "commit"})
SOURCE_REF_REPO_URL_MAX = 300
SOURCE_REF_COMMIT_MAX = 64

TITLE_MAX = 200

# Fields that carry the finding itself; a v2 envelope needs at least one.
FINDING_TEXT_FIELDS = ("title", "hypothesis", "claimed_impact", "mechanism")


@dataclass
class FindingEnvelopeV2(FindingEnvelope):
    """v1 fields plus the v2 additions. Validation never raises."""

    envelope_version: int = 2
    hypothesis: str = ""
    source_ref: dict | None = None
    code_context: str = ""
    program_rules: str = ""

    def validate(self) -> list[str]:
        errors = super().validate()
        if self.envelope_version != 2 or isinstance(self.envelope_version, bool):
            errors.append("envelope_version must be 2 for a v2 envelope")
        for name, max_length in V2_FIELD_MAX_LENGTHS.items():
            value = getattr(self, name)
            if not isinstance(value, str):
                errors.append(f"{name} must be a string")
            elif len(value) > max_length:
                errors.append(f"{name} length {len(value)} exceeds the maximum of {max_length}")
        errors.extend(_validate_source_ref(self.source_ref))
        if not any(
            isinstance(getattr(self, name), str) and getattr(self, name).strip()
            for name in FINDING_TEXT_FIELDS
        ):
            errors.append(
                "the envelope carries no finding text (title, hypothesis, "
                "claimed_impact or mechanism)"
            )
        return errors


def _validate_source_ref(source_ref) -> list[str]:
    if source_ref is None:
        return []
    if not isinstance(source_ref, dict):
        return ["source_ref must be an object with repo_url and commit"]
    errors = []
    unknown = set(source_ref) - SOURCE_REF_FIELDS
    if unknown:
        # Attacker-chosen key names are never echoed back; the count is safe.
        errors.append(f"source_ref contains unknown fields: {len(unknown)}")
    limits = {"repo_url": SOURCE_REF_REPO_URL_MAX, "commit": SOURCE_REF_COMMIT_MAX}
    for name, max_length in limits.items():
        value = source_ref.get(name)
        if not isinstance(value, str) or not value:
            errors.append(f"source_ref.{name} must be a non-empty string")
        elif len(value) > max_length:
            errors.append(
                f"source_ref.{name} length {len(value)} exceeds the maximum of {max_length}"
            )
    return errors


def from_v1(envelope: FindingEnvelope) -> FindingEnvelopeV2:
    """Map a v1 envelope into v2 deterministically (v2 fields at defaults)."""
    refs = envelope.evidence_refs
    return FindingEnvelopeV2(
        **{name: getattr(envelope, name) for name in V1_FIELDS if name != "evidence_refs"},
        evidence_refs=list(refs) if isinstance(refs, list) else refs,
    )


class BenchmarkRowError(ValueError):
    """A benchmark row is not a {id, title, hypothesis} object."""


def from_benchmark_row(row) -> FindingEnvelopeV2:
    """Map a benchmark input row ``{id, title, hypothesis, ...}`` into v2.

    Title and hypothesis are truncated to their limits (the benchmark
    corpus is not bound by the envelope limits); other row keys such as
    ``source_dir`` are ignored. The id stays with the caller.
    """
    if not isinstance(row, dict):
        raise BenchmarkRowError("benchmark row must be a JSON object")
    row_id = row.get("id")
    if not isinstance(row_id, (str, int)) or isinstance(row_id, bool) or row_id == "":
        raise BenchmarkRowError("benchmark row needs a string or integer id")
    title = row.get("title") or ""
    hypothesis = row.get("hypothesis") or ""
    if not isinstance(title, str) or not isinstance(hypothesis, str):
        raise BenchmarkRowError("benchmark row title and hypothesis must be strings")
    return FindingEnvelopeV2(
        title=title[:TITLE_MAX],
        hypothesis=hypothesis[: V2_FIELD_MAX_LENGTHS["hypothesis"]],
    )


def payload_version(payload: dict) -> int:
    """The envelope version a request payload declares or implies.

    An explicit ``envelope_version`` wins; otherwise any v2-only field
    makes the payload v2, and everything else is v1.
    """
    declared = payload.get(ENVELOPE_VERSION_FIELD)
    if declared is not None:
        if isinstance(declared, bool) or declared not in (1, 2):
            raise ValueError("envelope_version must be 1 or 2")
        if declared == 1 and V2_ONLY_FIELDS & set(payload):
            raise ValueError("v2 fields require envelope_version 2")
        return declared
    return 2 if V2_ONLY_FIELDS & set(payload) else 1


def load_payload(payload) -> FindingEnvelope:
    """Turn a parsed request body into a v1 or v2 envelope.

    Only shape problems that make an envelope impossible are raised here
    (non-object, unknown fields, malformed evidence_refs); field-level
    validation is the schema's job.
    """
    if not isinstance(payload, dict):
        raise ValueError("request body must be a JSON object")
    unknown = set(payload) - ALL_FIELDS
    if unknown:
        # The count is safe to report; the names are attacker-chosen.
        raise ValueError("the envelope contains unknown fields: " + str(len(unknown)))
    refs = payload.get("evidence_refs", [])
    if not isinstance(refs, list) or not all(isinstance(r, str) for r in refs):
        raise ValueError("evidence_refs must be a list of strings")
    version = payload_version(payload)
    common = {
        "title": payload.get("title", ""),
        "claimed_impact": payload.get("claimed_impact", ""),
        "mechanism": payload.get("mechanism", ""),
        "attacker_capability": payload.get("attacker_capability", ""),
        "preconditions": payload.get("preconditions", ""),
        "component_class": payload.get("component_class", ""),
        "scope_statement": payload.get("scope_statement", ""),
        "deployed_state_evidence": payload.get("deployed_state_evidence", ""),
        "public_rules_snapshot": payload.get("public_rules_snapshot", ""),
        "known_issue_declaration": payload.get("known_issue_declaration"),
        "reproduction_summary": payload.get("reproduction_summary", ""),
        "evidence_refs": list(refs),
        "code_excerpt": payload.get("code_excerpt"),
    }
    if version == 1:
        return FindingEnvelope(**common)
    return FindingEnvelopeV2(
        **common,
        hypothesis=payload.get("hypothesis", ""),
        source_ref=payload.get("source_ref"),
        code_context=payload.get("code_context", ""),
        program_rules=payload.get("program_rules", ""),
    )
