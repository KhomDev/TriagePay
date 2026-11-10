"""Ruleset loading and data-driven gate execution (fail closed).

The scanner executes compiled, hash-addressed ruleset artifacts produced
by aps-knowledge-compiler. Loading validates the artifact; an unknown
evaluator id, a malformed branch, or a missing gate raises RulesetError
and the scan refuses to run - never a silent default-pass.

Two artifact schema versions load:

- schema 1 (``rules-v0.synthetic.1``): the nine v1 gates and the six
  original evaluators; statuses Pass, Fail, NeedsEvidence.
- schema 2 (``rules-v1.public`` and the off-repo private ruleset): every
  gate names one public taxonomy category, branches may carry a penalty
  factor in [0, 1], the Held status is allowed, and the Stage A
  structured evaluators are available. Their parameters are validated at
  load time.
"""

import hashlib
import json
import os
import re
from pathlib import Path

from . import textmatch
from .normalize import NormalizedEnvelope
from .taxonomy import is_public_category

DEFAULT_RULESET_VERSION = "rules-v0.synthetic.1"
DEFAULT_RULESET_FILENAME = "rules-v0.synthetic.1.json"
V2_RULESET_VERSION = "rules-v1.public"
V2_RULESET_PATH_ENV = "APS_RULESET_V2_PATH"

# Committed artifacts by ruleset id.
BUNDLED_RULESETS = {
    "rules-v0.synthetic.1": "rules-v0.synthetic.1.json",
    "rules-v1.public": "rules-v1.public.json",
}

RULESET_SCHEMA_VERSION = 1
SUPPORTED_SCHEMA_VERSIONS = (1, 2)

V1_EVALUATORS = frozenset(
    {
        "field_empty",
        "field_present",
        "terms_any",
        "regex_search",
        "length_below",
        "starts_with",
    }
)

# Stage A structured evaluators (schema 2 only) and their parameters:
# name -> (required params, optional params). Mirrors
# knowledge_compiler.schema.EVALUATOR_PARAMS.
_LIST, _INT, _STR = "list", "int", "str"
V2_EVALUATOR_PARAMS = {
    "phrase_affirmed": ({"fields": _LIST, "phrases": _LIST}, {"window": _INT}),
    "privilege_required": (
        {
            "capability_fields": _LIST,
            "capability_terms": _LIST,
            "context_fields": _LIST,
            "context_phrases": _LIST,
            "escape_fields": _LIST,
            "escape_phrases": _LIST,
        },
        {"window": _INT},
    ),
    "quantified_impact": (
        {"fields": _LIST, "impact_terms": _LIST, "unit_terms": _LIST},
        {"absolute_phrases": _LIST, "window": _INT},
    ),
    "concession_present": ({"fields": _LIST, "phrases": _LIST}, {"rebuttal_markers": _LIST}),
    "rules_section_match": (
        {"section_markers": _LIST, "match_fields": _LIST},
        {"min_overlap": _INT},
    ),
    "evidence_format": ({"fields": _LIST, "expect": _STR}, {}),
}
EVIDENCE_EXPECTATIONS = frozenset({"placeholder", "onchain_ref", "pinned_source"})

# The fixed evaluator registry (mirrors knowledge_compiler.schema).
EVALUATORS = V1_EVALUATORS | frozenset(V2_EVALUATOR_PARAMS)

V1_STATUSES = ("Pass", "Fail", "NeedsEvidence")
V2_STATUSES = V1_STATUSES + ("Held",)


class RulesetError(ValueError):
    """The ruleset is invalid or references an unknown evaluator."""


def _rulesets_dir() -> Path:
    return Path(__file__).resolve().parent.parent / "rulesets"


def _default_ruleset_path() -> Path:
    return _rulesets_dir() / DEFAULT_RULESET_FILENAME


def _canonical(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def payload_hash(ruleset) -> str:
    """The sha256 of the canonical gates payload."""
    return hashlib.sha256(_canonical(ruleset["gates"]).encode("utf-8")).hexdigest()


def _validate_ruleset(artifact):
    if not isinstance(artifact, dict):
        raise RulesetError("ruleset must be a JSON object")
    schema_version = artifact.get("schema_version")
    if schema_version not in SUPPORTED_SCHEMA_VERSIONS or isinstance(schema_version, bool):
        raise RulesetError("unsupported ruleset schema version")
    for key in ("ruleset_id", "version", "payload_sha256", "gates"):
        if not artifact.get(key):
            raise RulesetError(f"ruleset is missing '{key}'")
    gates = artifact["gates"]
    if not isinstance(gates, list) or not gates:
        raise RulesetError("ruleset gates must be a non-empty list")
    statuses = V2_STATUSES if schema_version == 2 else V1_STATUSES
    seen = set()
    for gate in gates:
        if not isinstance(gate, dict):
            raise RulesetError("each gate must be an object")
        gate_id = gate.get("id")
        if not gate_id or gate_id in seen:
            raise RulesetError(f"invalid or duplicate gate id: {gate_id!r}")
        seen.add(gate_id)
        if schema_version == 2 and not is_public_category(gate.get("category")):
            raise RulesetError(f"gate {gate_id} must name a public taxonomy category")
        branches = gate.get("branches")
        if not isinstance(branches, list) or not branches:
            raise RulesetError(f"gate {gate_id} has no branches")
        for branch in branches:
            status = branch.get("status")
            if status not in statuses:
                raise RulesetError(f"gate {gate_id} has an illegal branch status")
            if "penalty" in branch:
                penalty = branch["penalty"]
                if (
                    schema_version != 2
                    or isinstance(penalty, bool)
                    or not isinstance(penalty, (int, float))
                    or not 0 <= penalty <= 1
                ):
                    raise RulesetError(f"gate {gate_id} has an illegal branch penalty")
            for condition in branch.get("when", []):
                _validate_condition(condition, f"{gate_id}", schema_version)
    if payload_hash(artifact) != artifact["payload_sha256"]:
        raise RulesetError("ruleset payload hash mismatch")
    return artifact


def _validate_params(evaluator, params, path):
    required, optional = V2_EVALUATOR_PARAMS[evaluator]
    if not isinstance(params, dict):
        raise RulesetError(f"{path}: {evaluator} needs params")
    unknown = set(params) - set(required) - set(optional)
    if unknown:
        raise RulesetError(f"{path}: {evaluator} has unknown params")
    for name, kind in {**required, **optional}.items():
        if name not in params:
            if name in required:
                raise RulesetError(f"{path}: {evaluator} is missing param '{name}'")
            continue
        value = params[name]
        if kind == _LIST:
            ok = isinstance(value, list) and all(isinstance(i, str) and i for i in value)
        elif kind == _INT:
            ok = isinstance(value, int) and not isinstance(value, bool) and value >= 0
        else:
            ok = isinstance(value, str) and bool(value)
        if not ok:
            raise RulesetError(f"{path}: {evaluator} param '{name}' has the wrong type")
    if evaluator == "evidence_format" and params["expect"] not in EVIDENCE_EXPECTATIONS:
        raise RulesetError(f"{path}: evidence_format has an unknown expectation")


def _validate_condition(condition, path, schema_version=1):
    if not isinstance(condition, dict):
        raise RulesetError(f"{path}: condition must be an object")
    if "any_of" in condition or "all_of" in condition:
        kind = "any_of" if "any_of" in condition else "all_of"
        children = condition.get(kind)
        if not isinstance(children, list) or not children:
            raise RulesetError(f"{path}: {kind} must be a non-empty list")
        for child in children:
            _validate_condition(child, path, schema_version)
        return
    evaluator = condition.get("eval")
    if evaluator not in EVALUATORS:
        raise RulesetError(f"{path}: unknown evaluator {evaluator!r} (fail closed)")
    if evaluator in V2_EVALUATOR_PARAMS:
        if schema_version != 2:
            raise RulesetError(f"{path}: {evaluator} needs a schema 2 ruleset (fail closed)")
        _validate_params(evaluator, condition.get("params"), path)


def load_ruleset(path=None):
    """Load and validate a ruleset artifact (default: the bundled one)."""
    ruleset_path = Path(path) if path else _default_ruleset_path()
    try:
        artifact = json.loads(ruleset_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as err:
        raise RulesetError(f"cannot read ruleset {ruleset_path.name}: {err}") from err
    return _validate_ruleset(artifact)


def load_bundled_ruleset(ruleset_id):
    """Load a committed ruleset by id (fail closed on unknown ids)."""
    filename = BUNDLED_RULESETS.get(ruleset_id)
    if filename is None:
        raise RulesetError("unknown bundled ruleset id")
    artifact = load_ruleset(_rulesets_dir() / filename)
    if artifact["ruleset_id"] != ruleset_id:
        raise RulesetError("bundled ruleset id does not match its file")
    return artifact


_DEFAULT = None
_V2_CACHE = {}


def default_ruleset():
    """The bundled default ruleset, loaded once and cached."""
    global _DEFAULT
    if _DEFAULT is None:
        _DEFAULT = load_ruleset()
    return _DEFAULT


def default_v2_ruleset(environ=None):
    """The v2 ruleset: APS_RULESET_V2_PATH when set (for example the
    off-repo private ruleset on the hosted service), else rules-v1.public.
    Either way it must be a schema 2 artifact."""
    environ = os.environ if environ is None else environ
    override = environ.get(V2_RULESET_PATH_ENV, "").strip()
    key = override or V2_RULESET_VERSION
    if key not in _V2_CACHE:
        artifact = load_ruleset(override) if override else load_bundled_ruleset(key)
        if artifact["schema_version"] != 2:
            raise RulesetError("the v2 engine needs a schema 2 ruleset")
        _V2_CACHE[key] = artifact
    return _V2_CACHE[key]


def _field_text(envelope: NormalizedEnvelope, field: str) -> str:
    value = getattr(envelope, field, None)
    if value is None:
        return ""
    if isinstance(value, dict):
        return " ".join(str(value[key]) for key in sorted(value))
    if isinstance(value, list):
        return " ".join(str(item) for item in value)
    return str(value)


def _combined_text(envelope: NormalizedEnvelope, condition) -> str:
    params = condition.get("params", {})
    fields = params.get("fields") or [condition.get("field")]
    join = params.get("join", "")
    return join.join(_field_text(envelope, field) for field in fields)


def _lines(envelope, fields) -> str:
    """Join fields with newlines so sentences never run across fields."""
    return "\n".join(_field_text(envelope, field) for field in fields)


def _evaluate_v2_leaf(evaluator, params, envelope) -> bool:
    window = params.get("window", textmatch.DEFAULT_NEGATION_WINDOW)
    if evaluator == "phrase_affirmed":
        return textmatch.any_phrase_affirmed(
            _lines(envelope, params["fields"]), params["phrases"], window
        )
    if evaluator == "privilege_required":
        return textmatch.privilege_required(
            _lines(envelope, params["capability_fields"]),
            params["capability_terms"],
            _lines(envelope, params["context_fields"]),
            params["context_phrases"],
            _lines(envelope, params["escape_fields"]),
            params["escape_phrases"],
            window,
        )
    if evaluator == "quantified_impact":
        return textmatch.quantified_impact(
            _lines(envelope, params["fields"]),
            params["impact_terms"],
            params["unit_terms"],
            params.get("absolute_phrases", []),
            params.get("window", 12),
        )
    if evaluator == "concession_present":
        return textmatch.concession_present(
            _lines(envelope, params["fields"]),
            params["phrases"],
            params.get("rebuttal_markers", textmatch.DEFAULT_REBUTTAL_MARKERS),
        )
    if evaluator == "rules_section_match":
        return textmatch.rules_section_match(
            _field_text(envelope, "program_rules"),
            params["section_markers"],
            _lines(envelope, params["match_fields"]),
            params.get("min_overlap", 1),
        )
    if evaluator == "evidence_format":
        expect = params["expect"]
        if expect == "pinned_source":
            return textmatch.pinned_source(getattr(envelope, "source_ref", None))
        text = _lines(envelope, params["fields"])
        if expect == "placeholder":
            return textmatch.evidence_placeholder(text)
        return textmatch.evidence_onchain_ref(text)
    raise RulesetError(f"unknown evaluator {evaluator!r} (fail closed)")


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
    if evaluator in V2_EVALUATOR_PARAMS:
        return _evaluate_v2_leaf(evaluator, params, envelope)
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
    return evaluate_gate_branch(record, envelope)[:2]


def evaluate_gate_branch(record, envelope: NormalizedEnvelope):
    """Like evaluate_gate, plus the matched branch's penalty (default 1.0)."""
    for branch in record["branches"]:
        if all(evaluate_condition(condition, envelope) for condition in branch.get("when", [])):
            return branch["status"], branch["reason"], float(branch.get("penalty", 1.0))
    raise RulesetError(f"gate {record['id']} has no matching branch (malformed ruleset)")
