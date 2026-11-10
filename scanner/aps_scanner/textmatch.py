"""Text primitives behind the Stage A evaluators (stdlib only, deterministic).

Every function takes plain strings and returns a bool, so the ruleset
module can wire them to envelope fields and the unit tests can exercise
them directly. The lexicons (phrases, terms, markers) live in the compiled
ruleset, not here: this module only knows HOW to match, never WHAT the
doctrine says.

Negation model: a phrase occurrence is negated when one of the
``NEGATORS`` appears within ``window`` tokens before it inside the same
clause. Clauses end at sentence punctuation and at contrastive
conjunctions ("but", "however", ...), so "no admin is needed, but the
owner must sign" negates "admin" and leaves "owner must" affirmed.
"""

import re

NEGATORS = frozenset(
    {
        "no",
        "not",
        "never",
        "without",
        "cannot",
        "none",
        "neither",
        "nor",
        "nothing",
        "nobody",
        "lacks",
        "lack",
    }
)
CONTRACTION_NEGATION = "n't"

CLAUSE_BREAK_WORDS = frozenset(
    {"but", "however", "although", "though", "whereas", "except", "yet", "while"}
)

DEFAULT_NEGATION_WINDOW = 4

_APOSTROPHES = str.maketrans({"’": "'", "‘": "'", "`": "'"})
_TOKEN_PATTERN = re.compile(r"[a-z0-9]+(?:'[a-z]+)?|%|\$")
# Sentence boundaries: ., !, ?, ; and newlines, but not decimal points or
# dotted identifiers ("1.5", "a.b").
_SENTENCE_SPLIT = re.compile(r"(?<![0-9a-z])[.!?;]+|[.!?;]+(?![0-9a-z])|\n+")


def prepare(text: str) -> str:
    """Lowercase and fold typographic apostrophes."""
    return (text or "").translate(_APOSTROPHES).lower()


def tokens(text: str) -> list:
    """Lowercase word tokens ("doesn't" stays one token)."""
    return _TOKEN_PATTERN.findall(prepare(text))


def sentences(text: str) -> list:
    """Split text into sentences (no empty items)."""
    return [part.strip() for part in _SENTENCE_SPLIT.split(prepare(text)) if part.strip()]


def _is_negator(token: str) -> bool:
    return token in NEGATORS or token.endswith(CONTRACTION_NEGATION)


def _clauses(sentence_tokens: list) -> list:
    """Split one sentence's tokens at contrastive conjunctions."""
    clauses, current = [], []
    for token in sentence_tokens:
        if token in CLAUSE_BREAK_WORDS:
            if current:
                clauses.append(current)
            current = []
            continue
        current.append(token)
    if current:
        clauses.append(current)
    return clauses


def _find(sequence: list, needle: list) -> list:
    """Start indexes of every occurrence of needle in sequence."""
    size = len(needle)
    if not size:
        return []
    return [i for i in range(len(sequence) - size + 1) if sequence[i : i + size] == needle]


def _occurrences(text: str, phrase: str, window: int):
    """Yield (clause_tokens, start_index, negated) for each phrase occurrence."""
    needle = tokens(phrase)
    for sentence in sentences(text):
        for clause in _clauses(tokens(sentence)):
            for start in _find(clause, needle):
                before = clause[max(0, start - window) : start]
                yield clause, start, any(_is_negator(token) for token in before)


def phrase_affirmed(text: str, phrase: str, window: int = DEFAULT_NEGATION_WINDOW) -> bool:
    """True when the phrase occurs at least once without a preceding negator."""
    return any(not negated for _clause, _start, negated in _occurrences(text, phrase, window))


def any_phrase_affirmed(text: str, phrases, window: int = DEFAULT_NEGATION_WINDOW) -> bool:
    return any(phrase_affirmed(text, phrase, window) for phrase in phrases)


def privilege_required(
    capability_text: str,
    capability_terms,
    context_text: str,
    context_phrases,
    escape_text: str,
    escape_phrases,
    window: int = DEFAULT_NEGATION_WINDOW,
) -> bool:
    """Precondition-privilege classification.

    Privileged when a role term is affirmed in the attacker-capability text
    (bare role words: the field describes the attacker) or a requirement
    phrase is affirmed in the wider context (preconditions, hypothesis),
    AND no permissionless escape is affirmed. Negation applies to both
    sides: "no admin role is needed" is not privileged, and "not
    permissionless" is not an escape.
    """
    privileged = any_phrase_affirmed(
        capability_text, capability_terms, window
    ) or any_phrase_affirmed(context_text, context_phrases, window)
    if not privileged:
        return False
    return not any_phrase_affirmed(escape_text, escape_phrases, window)


_NUMBER = r"\d[\d,_]*(?:\.\d+)?(?:e\d+)?"
_MAGNITUDE = r"(?:\s?(?:k|m|b|thousand|million|billion|mn|bn))?"
_CURRENCY_QUANTITY = re.compile(r"\$\s?" + _NUMBER + _MAGNITUDE + r"\b")
_PERCENT_QUANTITY = re.compile(_NUMBER + r"\s?(?:%|percent\b|bps\b|basis points\b)")
_POWER_QUANTITY = re.compile(r"\b10\s?\*\*\s?\d+|\b1e\d+\b")


def _unit_quantity_pattern(units) -> re.Pattern:
    alternatives = "|".join(sorted((re.escape(unit) for unit in units), key=len, reverse=True))
    return re.compile(r"\b" + _NUMBER + _MAGNITUDE + r"\s?(?:" + alternatives + r")\b")


def _token_index(sentence: str, char_offset: int) -> int:
    return len(_TOKEN_PATTERN.findall(sentence[:char_offset]))


def quantified_impact(
    text: str,
    impact_terms,
    unit_terms,
    absolute_phrases,
    window: int = 12,
    negation_window: int = DEFAULT_NEGATION_WINDOW,
) -> bool:
    """True when an amount, percentage or token quantity is bound to an
    affirmed impact verb: both in the same sentence, within ``window``
    tokens of each other. An affirmed absolute phrase ("all deposits")
    in a sentence with an affirmed impact verb also counts.
    """
    unit_pattern = _unit_quantity_pattern(unit_terms) if unit_terms else None
    for sentence in sentences(text):
        impact_positions = []
        for term in impact_terms:
            for clause, start, negated in _occurrences(sentence, term, negation_window):
                if not negated:
                    # Position within the sentence: re-find the clause offset.
                    impact_positions.append(_clause_offset(sentence, clause) + start)
        if not impact_positions:
            continue
        if any(phrase_affirmed(sentence, phrase, negation_window) for phrase in absolute_phrases):
            return True
        patterns = [_CURRENCY_QUANTITY, _PERCENT_QUANTITY, _POWER_QUANTITY]
        if unit_pattern is not None:
            patterns.append(unit_pattern)
        for pattern in patterns:
            for match in pattern.finditer(sentence):
                position = _token_index(sentence, match.start())
                if any(abs(position - impact) <= window for impact in impact_positions):
                    return True
    return False


def _clause_offset(sentence: str, clause: list) -> int:
    """Token offset of a clause inside its sentence (first exact match)."""
    sentence_tokens = tokens(sentence)
    starts = _find(sentence_tokens, clause)
    return starts[0] if starts else 0


DEFAULT_REBUTTAL_MARKERS = (
    "however",
    "but",
    "although",
    "even if",
    "even though",
    "claims",
    "claimed",
    "argues",
    "argued",
    "contrary",
    "incorrectly",
    "wrongly",
    "despite",
    "would be",
)


def concession_present(
    text: str,
    phrases,
    rebuttal_markers=DEFAULT_REBUTTAL_MARKERS,
    window: int = DEFAULT_NEGATION_WINDOW,
) -> bool:
    """Self-undermining admission detection.

    True when a concession phrase is affirmed in a sentence that carries no
    rebuttal marker: "Note that no funds are lost" concedes; "The sponsor
    claims no funds are lost, but the PoC drains 10 ETH" does not.
    """
    for sentence in sentences(text):
        if not any(phrase_affirmed(sentence, phrase, window) for phrase in phrases):
            continue
        sentence_tokens = tokens(sentence)
        rebutted = any(_find(sentence_tokens, tokens(marker)) for marker in rebuttal_markers)
        if not rebutted:
            return True
    return False


STOPWORDS = frozenset(
    {
        "the",
        "and",
        "for",
        "with",
        "from",
        "that",
        "this",
        "are",
        "any",
        "all",
        "not",
        "can",
        "via",
        "its",
        "into",
        "when",
        "will",
        "has",
        "have",
        "been",
        "was",
        "were",
        "which",
        "their",
        "they",
        "than",
        "then",
        "there",
        "these",
        "those",
        "issue",
        "issues",
        "contract",
        "contracts",
        "function",
        "functions",
        "user",
        "users",
        "attacker",
        "loss",
        "funds",
        "path",
        "scope",
        "out",
    }
)


def significant_tokens(text: str) -> set:
    """Distinct lowercase tokens of 3+ characters that are not stopwords."""
    return {t for t in tokens(text) if len(t) >= 3 and t not in STOPWORDS and not t.isdigit()}


def _rules_segments(rules: str) -> list:
    """Split program rules into lines (multi-line text) or sentences."""
    text = prepare(rules)
    if "\n" in text.strip():
        return [line.strip() for line in text.split("\n")]
    return sentences(text) or [text]


def _is_header(line: str) -> bool:
    return line.startswith("#") or line.rstrip().endswith(":")


def rules_sections(rules: str, section_markers) -> list:
    """The parts of the program rules introduced by one of the markers.

    A segment containing an affirmed marker opens a section; on multi-line
    rules the section continues through following lines until a blank line
    or a new header line that carries no marker. Single-line rules are split
    into sentences, and each marked sentence is its own section.
    """
    if "\n" not in prepare(rules).strip():
        return [
            segment
            for segment in _rules_segments(rules)
            if any(phrase_affirmed(segment, marker) for marker in section_markers)
        ]
    sections, current = [], None
    for segment in _rules_segments(rules):
        has_marker = any(phrase_affirmed(segment, marker) for marker in section_markers)
        if has_marker:
            if current is not None:
                sections.append(current)
            current = [segment]
            continue
        if current is None:
            continue
        if not segment or _is_header(segment):
            sections.append(current)
            current = None
            continue
        current.append(segment)
    if current is not None:
        sections.append(current)
    return [" ".join(section) for section in sections]


def rules_section_match(rules: str, section_markers, match_text: str, min_overlap: int) -> bool:
    """Scope cross-check: True when a marked section of the program rules
    shares at least ``min_overlap`` significant tokens with match_text."""
    if not rules or not match_text or min_overlap < 1:
        return False
    wanted = significant_tokens(match_text)
    if not wanted:
        return False
    for section in rules_sections(rules, section_markers):
        present = set(tokens(section))
        if len(wanted & present) >= min_overlap:
            return True
    return False


# --- evidence-reference format checks ---------------------------------------

_HEX_REF = re.compile(r"(?<![0-9a-z])0x([0-9a-f]+)(?![0-9a-z])")
_PLACEHOLDER_MARKERS = re.compile(
    "0x\\.\\.\\.|0x…|" r"<address>|<tx>|\btbd\b|\btodo\b|\bxxxx+\b|\bplaceholder\b"
)
_BASE58_TX = re.compile(r"(?<![1-9a-km-zA-HJ-NP-Z])[1-9A-HJ-NP-Za-km-z]{43,44}(?![1-9A-Za-z])")
_EXPLORER_URL = re.compile(
    r"https://[a-z0-9.-]+\.[a-z]{2,}/(?:tx|txns|transactions|address|accounts?|blocks?)/[^\s/]{6,}"
)
_REPO_URL = re.compile(r"https://[a-z0-9.-]+\.[a-z]{2,}/\S+")
_COMMIT = re.compile(r"[0-9a-f]{7,40}")
_SEQUENTIAL = "0123456789abcdef"
VALID_HEX_LENGTHS = (40, 64)


def _low_entropy(hex_digits: str) -> bool:
    """Obvious fabrications: few distinct digits or a counting run."""
    if len(set(hex_digits)) <= 3:
        return True
    doubled = _SEQUENTIAL * 2
    run = "1234567890abcdef"
    for pattern in (doubled, run * 5):
        if hex_digits in pattern or pattern.startswith(hex_digits):
            return True
    # Repetition of a short unit ("deadbeef" * 5).
    for unit in range(1, min(9, len(hex_digits))):
        if len(hex_digits) % unit == 0 and hex_digits == hex_digits[:unit] * (
            len(hex_digits) // unit
        ):
            return True
    return False


def hex_refs(text: str) -> list:
    return _HEX_REF.findall(prepare(text))


def evidence_placeholder(text: str) -> bool:
    """True when the text carries a malformed or fabricated-looking reference.

    Malformed: an 0x hex run whose length is neither an address (40) nor a
    hash (64). Fabricated-looking: a well-sized run with obvious low
    entropy, or a placeholder marker ("0x...", "<address>", "TBD").
    """
    lowered = prepare(text)
    if _PLACEHOLDER_MARKERS.search(lowered):
        return True
    for digits in _HEX_REF.findall(lowered):
        if len(digits) not in VALID_HEX_LENGTHS or _low_entropy(digits):
            return True
    return False


def evidence_onchain_ref(text: str) -> bool:
    """True when the text carries at least one well-formed on-chain reference:
    a 40-hex address or 64-hex hash, a 43-44 character base58 transaction
    hash, or an explorer URL naming a tx, address, account or block.
    Format-level only: nothing here proves the reference exists on chain.
    """
    raw = text or ""
    lowered = prepare(raw)
    for digits in _HEX_REF.findall(lowered):
        if len(digits) in VALID_HEX_LENGTHS and not _low_entropy(digits):
            return True
    if _BASE58_TX.search(raw):
        return True
    return _EXPLORER_URL.search(lowered) is not None


def pinned_source(source_ref) -> bool:
    """True when source_ref is a well-formed https repository URL plus a
    7-40 hex commit that is not an obvious fabrication."""
    if not isinstance(source_ref, dict):
        return False
    repo_url = prepare(source_ref.get("repo_url", "")).strip()
    commit = prepare(source_ref.get("commit", "")).strip()
    if not _REPO_URL.fullmatch(repo_url) or not _COMMIT.fullmatch(commit):
        return False
    return not _low_entropy(commit)
