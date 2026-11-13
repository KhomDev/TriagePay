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


# Markdown emphasis and strike-through marks a payload can wrap around an
# instruction ("**ignore** previous instructions", "i_g_n_o_r_e").
_EMPHASIS = re.compile(r"[*_~]+")


def _detection_view(text: str) -> str:
    """The text the flag inspects: NFKC-folded (fullwidth and compatibility
    forms become ASCII), every format character (Unicode Cf, not only the
    four zero-width ones) removed, emphasis marks removed, whitespace
    collapsed. Used only for flagging; the normalized fields are unchanged.
    """
    folded = unicodedata.normalize("NFKC", text)
    visible = "".join(ch for ch in folded if unicodedata.category(ch) != "Cf")
    return _WHITESPACE_RUN.sub(" ", _EMPHASIS.sub("", visible))


def _contains_embedded_instructions(text: str) -> bool:
    if not text:
        return False
    if FENCED_SYSTEM_PATTERN.search(text) is not None:
        return True
    for view in (text, _detection_view(text)):
        if any(pattern.search(view) for pattern in INSTRUCTION_PATTERNS):
            return True
    return False


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


# --- envelope v2 -----------------------------------------------------------

_INLINE_SPACE_RUN = re.compile(r"[^\S\n]+")
_BLANK_LINE_RUN = re.compile(r"\n{3,}")
_KEPT_CONTROLS = ("\n", "\t")


def _replace_controls_keep_lines(text: str) -> str:
    return "".join(
        REPLACEMENT if unicodedata.category(ch) == "Cc" and ch not in _KEPT_CONTROLS else ch
        for ch in text
    )


def _scrub_multiline(text: str) -> str:
    """Like _scrub_text but line structure survives.

    CRLF and CR fold to LF, inline whitespace runs collapse to one space,
    each line is stripped, and runs of blank lines collapse to one. Used
    for program_rules, whose section headers and list items carry meaning.
    """
    folded = unicodedata.normalize("NFC", text).replace("\r\n", "\n").replace("\r", "\n")
    cleaned = _strip_zero_width(_replace_controls_keep_lines(folded))
    lines = [_INLINE_SPACE_RUN.sub(" ", line).strip() for line in cleaned.split("\n")]
    return _BLANK_LINE_RUN.sub("\n\n", "\n".join(lines)).strip("\n")


def _scrub_code_keep_lines(text: str) -> str:
    """Code context: verbatim apart from control chars (LF and TAB kept)."""
    folded = unicodedata.normalize("NFC", text).replace("\r\n", "\n").replace("\r", "\n")
    return _strip_zero_width(_replace_controls_keep_lines(folded))


@dataclass
class NormalizedEnvelopeV2(NormalizedEnvelope):
    """Normalized v2 envelope: the v1 fields plus the v2 additions."""

    envelope_version: int = 2
    hypothesis: str = ""
    source_ref: dict | None = None
    code_context: str = ""
    program_rules: str = ""


def _normalize_source_ref(source_ref):
    if source_ref is None:
        return None
    return {
        "repo_url": _scrub_text(source_ref.get("repo_url", "")),
        "commit": _scrub_text(source_ref.get("commit", "")).lower(),
    }


def normalize_envelope_v2(envelope) -> NormalizedEnvelopeV2:
    """Deterministic normalized form of a (validated) v2 envelope.

    The injection flag covers every v1 field plus hypothesis, code_context,
    program_rules and source_ref. It stays advisory: no gate reads it.
    """
    base = normalize_envelope(envelope)
    base_fields = {
        name: getattr(base, name) for name in NormalizedEnvelope.__dataclass_fields__
    }
    normalized = NormalizedEnvelopeV2(
        **base_fields,
        hypothesis=_scrub_text(envelope.hypothesis),
        source_ref=_normalize_source_ref(envelope.source_ref),
        code_context=_scrub_code_keep_lines(envelope.code_context),
        program_rules=_scrub_multiline(envelope.program_rules),
    )
    extra = [normalized.hypothesis, normalized.code_context, normalized.program_rules]
    if normalized.source_ref is not None:
        extra.extend(normalized.source_ref.values())
    normalized.contains_embedded_instructions = base.contains_embedded_instructions or any(
        _contains_embedded_instructions(text) for text in extra
    )
    return normalized
