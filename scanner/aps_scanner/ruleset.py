"""Ruleset loading and data-driven gate execution (fail closed).

The scanner executes compiled, hash-addressed ruleset artifacts produced
by aps-knowledge-compiler. Loading validates the artifact; an unknown
evaluator id, a malformed branch, or a missing gate raises RulesetError
and the scan refuses to run - never a silent default-pass.
"""

import hashlib
import json
import re
from pathlib import Path

from .normalize import NormalizedEnvelope

DEFAULT_RULESET_VERSION = "rules-v0.synthetic.1"
DEFAULT_RULESET_FILENAME = "rules-v0.synthetic.1.json"

RULESET_SCHEMA_VERSION = 1

# The fixed evaluator registry (mirrors knowledge_compiler.schema).
EVALUATORS = frozenset(
    {
        "field_empty",
        "field_present",
        "terms_any",
        "regex_search",
        "length_below",
        "starts_with",
    }
)


class RulesetError(ValueError):
    """The ruleset is invalid or references an unknown evaluator."""


def _default_ruleset_path() -> Path:
    return Path(__file__).resolve().parent.parent / "rulesets" / DEFAULT_RULESET_FILENAME


def _canonical(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def payload_hash(ruleset) -> str:
    """The sha256 of the canonical gates payload."""
    return hashlib.sha256(_canonical(ruleset["gates"]).encode("utf-8")).hexdigest()


def _validate_ruleset(artifact):
    if not isinstance(artifact, dict):
        raise RulesetError("ruleset must be a JSON object")
    if artifact.get("schema_version") != RULESET_SCHEMA_VERSION:
        raise RulesetError("unsupported ruleset schema version")
    for key in ("ruleset_id", "version", "payload_sha256", "gates"):
        if not artifact.get(key):
            raise RulesetError(f"ruleset is missing '{key}'")
    gates = artifact["gates"]
    if not isinstance(gates, list) or not gates:
        raise RulesetError("ruleset gates must be a non-empty list")
    seen = set()
    for gate in gates:
        if not isinstance(gate, dict):
            raise RulesetError("each gate must be an object")
        gate_id = gate.get("id")
        if not gate_id or gate_id in seen:
            raise RulesetError(f"invalid or duplicate gate id: {gate_id!r}")
        seen.add(gate_id)
        branches = gate.get("branches")
        if not isinstance(branches, list) or not branches:
            raise RulesetError(f"gate {gate_id} has no branches")
        for branch in branches:
            status = branch.get("status")
            if status not in ("Pass", "Fail", "NeedsEvidence"):
                raise RulesetError(f"gate {gate_id} has an illegal branch status")
            for condition in branch.get("when", []):
                _validate_condition(condition, f"{gate_id}")
    if payload_hash(artifact) != artifact["payload_sha256"]:
        raise RulesetError("ruleset payload hash mismatch")
    return artifact


def _validate_condition(condition, path):
    if not isinstance(condition, dict):
        raise RulesetError(f"{path}: condition must be an object")
    if "any_of" in condition or "all_of" in condition:
        kind = "any_of" if "any_of" in condition else "all_of"
        children = condition.get(kind)
        if not isinstance(children, list) or not children:
            raise RulesetError(f"{path}: {kind} must be a non-empty list")
        for child in children:
            _validate_condition(child, path)
        return
    evaluator = condition.get("eval")
    if evaluator not in EVALUATORS:
        raise RulesetError(f"{path}: unknown evaluator {evaluator!r} (fail closed)")


def load_ruleset(path=None):
    """Load and validate a ruleset artifact (default: the bundled one)."""
    ruleset_path = Path(path) if path else _default_ruleset_path()
    try:
        artifact = json.loads(ruleset_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as err:
        raise RulesetError(f"cannot read ruleset {ruleset_path.name}: {err}") from err
    return _validate_ruleset(artifact)


_DEFAULT = None


def default_ruleset():
    """The bundled default ruleset, loaded once and cached."""
    global _DEFAULT
    if _DEFAULT is None:
        _DEFAULT = load_ruleset()
    return _DEFAULT


def _field_text(envelope: NormalizedEnvelope, field: str) -> str:
    value = getattr(envelope, field)
    if value is None:
        return ""
    if isinstance(value, list):
        return " ".join(str(item) for item in value)
    return str(value)


def _combined_text(envelope: NormalizedEnvelope, condition) -> str:
    params = condition.get("params", {})
    fields = params.get("fields") or [condition.get("field")]
    join = params.get("join", "")
    return join.join(_field_text(envelope, field) for field in fields)


def _evaluate_leaf(condition, envelope: NormalizedEnvelope) -> bool:
    evaluator = condition["eval"]
    field = condition.get("field")
    params = condition.get("params", {})
    if evaluator == "field_empty":
        return _field_text(envelope, field) == ""
    if evaluator == "field_present":
        return getattr(envelope, field) is not None and _field_text(envelope, field) != ""
    if evaluator == "terms_any":
        text = _combined_text(envelope, condition).lower()
        return any(term in text for term in params.get("terms", []))
    if evaluator == "regex_search":
        pattern = re.compile(params.get("pattern", ""), re.IGNORECASE)
        return pattern.search(_combined_text(envelope, condition)) is not None
    if evaluator == "length_below":
        return len(_field_text(envelope, field)) < params.get("min", 0)
    if evaluator == "starts_with":
        text = _field_text(envelope, field)
        return any(text.startswith(prefix) for prefix in params.get("prefixes", []))
    raise RulesetError(f"unknown evaluator {evaluator!r} (fail closed)")


def evaluate_condition(condition, envelope: NormalizedEnvelope) -> bool:
    """Evaluate one condition node against a normalized envelope."""
    if "any_of" in condition:
        return any(evaluate_condition(child, envelope) for child in condition["any_of"])
    if "all_of" in condition:
        return all(evaluate_condition(child, envelope) for child in condition["all_of"])
    result = _evaluate_leaf(condition, envelope)
    return not result if condition.get("negate") else result


def evaluate_gate(record, envelope: NormalizedEnvelope):
    """Execute one gate record: first matching branch wins.

    Returns (status, reason) as raw strings; the gates module wraps them
    in the typed GateResult (kept separate to avoid an import cycle).
    """
    for branch in record["branches"]:
        if all(evaluate_condition(condition, envelope) for condition in branch.get("when", [])):
            return branch["status"], branch["reason"]
    raise RulesetError(f"gate {record['id']} has no matching branch (malformed ruleset)")
