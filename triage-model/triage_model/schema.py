"""The Stage B output schema and its strict validator.

The model must answer with exactly one JSON object:

- ``bountiable_probability``: number in [0, 1];
- ``severity_call``: one of invalid, info/low, medium, high;
- ``taxonomy_hits``: distinct public taxonomy ids only;
- ``missing_evidence``: distinct ids from MISSING_EVIDENCE;
- ``public_rationale``: at most 600 characters.

Nothing else is accepted: unknown keys, wrong types, private ids or an
over-long rationale are schema violations (one retry, then fail closed).
"""

import json
import math
import re

from aps_scanner.taxonomy import PUBLIC_CATEGORIES

SEVERITY_CALLS = ("invalid", "info/low", "medium", "high")
MISSING_EVIDENCE = (
    "deployment-reference",
    "reproduction-steps",
    "quantified-impact",
    "attacker-model",
    "scope-confirmation",
    "code-reference",
    "program-rules",
)
RATIONALE_MAX = 600
OUTPUT_FIELDS = (
    "bountiable_probability",
    "severity_call",
    "taxonomy_hits",
    "missing_evidence",
    "public_rationale",
)

OUTPUT_SCHEMA = {
    "type": "object",
    "additionalProperties": False,
    "required": list(OUTPUT_FIELDS),
    "properties": {
        "bountiable_probability": {"type": "number", "minimum": 0, "maximum": 1},
        "severity_call": {"type": "string", "enum": list(SEVERITY_CALLS)},
        "taxonomy_hits": {
            "type": "array",
            "items": {"type": "string", "enum": list(PUBLIC_CATEGORIES)},
            "uniqueItems": True,
        },
        "missing_evidence": {
            "type": "array",
            "items": {"type": "string", "enum": list(MISSING_EVIDENCE)},
            "uniqueItems": True,
        },
        "public_rationale": {"type": "string", "maxLength": RATIONALE_MAX},
    },
}

_FENCE = re.compile(r"^```(?:json)?\s*\n(.*)\n```$", re.DOTALL)


def parse_output(content):
    """Parse and validate model content. Returns (value, errors)."""
    if not isinstance(content, str):
        return None, ["the model reply is not text"]
    text = content.strip()
    fenced = _FENCE.match(text)
    if fenced:
        text = fenced.group(1).strip()
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        return None, ["the model reply is not JSON"]
    errors = validate_output(value)
    return (value if not errors else None), errors


def _distinct_from(value, allowed):
    return (
        isinstance(value, list)
        and all(isinstance(item, str) and item in allowed for item in value)
        and len(set(value)) == len(value)
    )


def validate_output(value) -> list:
    """Error strings (fixed texts, never echoing model content)."""
    if not isinstance(value, dict):
        return ["the reply must be a JSON object"]
    errors = []
    if set(value) != set(OUTPUT_FIELDS):
        errors.append("the reply must carry exactly the schema fields")
    probability = value.get("bountiable_probability")
    if (
        isinstance(probability, bool)
        or not isinstance(probability, (int, float))
        or not math.isfinite(probability)
        or not 0 <= probability <= 1
    ):
        errors.append("bountiable_probability must be a number in [0, 1]")
    if value.get("severity_call") not in SEVERITY_CALLS:
        errors.append("severity_call is not an allowed value")
    if not _distinct_from(value.get("taxonomy_hits"), PUBLIC_CATEGORIES):
        errors.append("taxonomy_hits must be distinct public category ids")
    if not _distinct_from(value.get("missing_evidence"), MISSING_EVIDENCE):
        errors.append("missing_evidence must be distinct allowed ids")
    rationale = value.get("public_rationale")
    if not isinstance(rationale, str) or len(rationale) > RATIONALE_MAX:
        errors.append(f"public_rationale must be a string of at most {RATIONALE_MAX} characters")
    return errors
