"""Input normalization and embedded-instruction flagging.

Every field is treated as hostile data. Normalization is deterministic and
idempotent. The embedded-instructions flag is ADVISORY ONLY: it never changes
any gate result, decision, or confidence band (enforced by tests).
"""

import re
import unicodedata
from dataclasses import dataclass, field

from .schema import FindingEnvelope

ZERO_WIDTH_CHARS = ("\u200b", "\u200c", "\u200d", "\ufeff")
REPLACEMENT = "\ufffd"

_WHITESPACE_RUN = re.compile(r"\s+")

# Case-insensitive markers of instructions embedded in the payload text.
INSTRUCTION_PATTERNS = (
    re.compile(r"ignore (all )?(previous|prior) instructions", re.IGNORECASE),
    re.compile(r"disregard (all )?(previous|prior)", re.IGNORECASE),
    re.compile(r"system prompt", re.IGNORECASE),
    re.compile(r"you are now", re.IGNORECASE),
    re.compile(r"act as (a |an )?(different|new)", re.IGNORECASE),
    re.compile(r"<\|im_start\|>", re.IGNORECASE),
    re.compile(r"todo:? for the (scanner|model|agent)", re.IGNORECASE),
)

# A fenced code block whose info string contains "system": triple backticks
# followed on the same line by "system" (with or without other info text).
FENCED_SYSTEM_PATTERN = re.compile(r"```[^`\n]*system", re.IGNORECASE)


def _replace_controls(text: str) -> str:
    # C0 (U+0000-U+001F) and C1 (U+007F-U+009F) share Unicode category Cc.
    return "".join(REPLACEMENT if unicodedata.category(ch) == "Cc" else ch for ch in text)


def _strip_zero_width(text: str) -> str:
    return "".join(ch for ch in text if ch not in ZERO_WIDTH_CHARS)


def _scrub_text(text: str) -> str:
    """NFC-fold, replace control chars, drop zero-width chars, collapse whitespace."""
    folded = unicodedata.normalize("NFC", text)
    cleaned = _strip_zero_width(_replace_controls(folded))
    return _WHITESPACE_RUN.sub(" ", cleaned)


def _scrub_code(text: str) -> str:
    """Like _scrub_text but verbatim apart from control-char replacement."""
    folded = unicodedata.normalize("NFC", text)
    return _strip_zero_width(_replace_controls(folded))


def _contains_embedded_instructions(text: str) -> bool:
    if not text:
        return False
    if any(pattern.search(text) for pattern in INSTRUCTION_PATTERNS):
        return True
    return FENCED_SYSTEM_PATTERN.search(text) is not None


@dataclass
class NormalizedEnvelope:
    """Normalized copy of a FindingEnvelope plus the advisory injection flag."""

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
    contains_embedded_instructions: bool = False


_TEXT_FIELDS = (
    "title",
    "claimed_impact",
    "mechanism",
    "attacker_capability",
    "preconditions",
    "component_class",
    "scope_statement",
    "deployed_state_evidence",
    "public_rules_snapshot",
    "reproduction_summary",
)


def normalize_envelope(envelope: FindingEnvelope) -> NormalizedEnvelope:
    """Produce the deterministic normalized form of the envelope."""
    refs = envelope.evidence_refs if isinstance(envelope.evidence_refs, list) else []
    normalized = NormalizedEnvelope(
        **{name: _scrub_text(getattr(envelope, name)) for name in _TEXT_FIELDS},
        known_issue_declaration=(
            _scrub_text(envelope.known_issue_declaration)
            if envelope.known_issue_declaration is not None
            else None
        ),
        evidence_refs=[_scrub_text(ref) for ref in refs],
        code_excerpt=(
            _scrub_code(envelope.code_excerpt) if envelope.code_excerpt is not None else None
        ),
    )

    texts = [getattr(normalized, name) for name in _TEXT_FIELDS]
    if normalized.known_issue_declaration is not None:
        texts.append(normalized.known_issue_declaration)
    if normalized.code_excerpt is not None:
        texts.append(normalized.code_excerpt)
    texts.extend(normalized.evidence_refs)
    normalized.contains_embedded_instructions = any(
        _contains_embedded_instructions(text) for text in texts
    )
    return normalized
