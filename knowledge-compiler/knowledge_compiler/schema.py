"""Schema validation for knowledge sources and compiled artifacts.

Fail closed: any unknown field, unknown evaluator id, illegal branch
status, or malformed structure is a SchemaError, never a silent default.

Two schema versions exist:

- 1: the original nine-gate format (``rules-v0.synthetic.1``);
- 2: the triage-engine v2 format (``rules-v1.public`` and the off-repo
  private ruleset). Every gate names one public taxonomy category,
  branches may carry a ``penalty`` factor in [0, 1], the ``Held`` status is
  allowed, and the Stage A structured evaluators may be used (their
  parameters are validated here and again by the scanner at load time).
"""

SCHEMA_VERSIONS = (1, 2)

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

# Stage A structured evaluators: name -> (required params, optional params).
# Mirrors aps_scanner.ruleset.V2_EVALUATOR_PARAMS (a cross-package test
# keeps the two in sync).
_LIST, _INT, _STR = "list", "int", "str"
EVALUATOR_PARAMS = {
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
# Params naming envelope fields (validated against ENVELOPE_FIELDS).
FIELD_LIST_PARAMS = frozenset(
    {"fields", "capability_fields", "context_fields", "escape_fields", "match_fields"}
)

# The fixed evaluator registry. aps-scanner implements exactly these
# evaluators; a ruleset referencing anything else is rejected at load
# time (fail closed) rather than default-passed.
EVALUATORS = V1_EVALUATORS | frozenset(EVALUATOR_PARAMS)

BRANCH_STATUSES = frozenset({"Pass", "Fail", "NeedsEvidence"})
V2_BRANCH_STATUSES = BRANCH_STATUSES | {"Held"}

MIGRATION_FAMILIES = frozenset(
    {
        "realism",
        "deployed_state",
        "public_evidence",
        "adversarial_review",
        "stage_a",
    }
)

# The public taxonomy (mirrors aps_scanner.taxonomy.PUBLIC_CATEGORIES).
PUBLIC_CATEGORIES = (
    "privileged-actor-required",
    "no-quantified-impact",
    "self-harm-only",
    "theoretical-only",
    "design-intent-likely",
    "out-of-scope-component",
    "known-issue-declared",
    "concession-contradicts-impact",
    "unverified-deployment-evidence",
)

# Envelope fields the evaluators may reference. Kept in sync with
# aps_scanner.schema.FindingEnvelope and aps_scanner.envelope_v2.
V1_ENVELOPE_FIELDS = frozenset(
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
ENVELOPE_FIELDS = V1_ENVELOPE_FIELDS | {
    "hypothesis",
    "source_ref",
    "code_context",
    "program_rules",
}

GATE_FIELDS = frozenset({"id", "hard", "public_explanation", "branches"})
V2_GATE_FIELDS = GATE_FIELDS | {"category"}
BRANCH_FIELDS = frozenset({"when", "status", "reason"})
V2_BRANCH_FIELDS = BRANCH_FIELDS | {"penalty"}
MIGRATION_FIELDS = frozenset(
    {"id", "family", "gate_ids", "public_explanation", "introduced_version"}
)
SOURCE_FIELDS = frozenset({"schema_version", "label", "gates", "migrations"})
ARTIFACT_FIELDS = frozenset(
    {
        "schema_version",
        "ruleset_id",
        "version",
        "created_unix",
        "payload_sha256",
        "counts",
        "provenance",
        "gates",
        "migrations",
    }
)


class SchemaError(ValueError):
    """A source or artifact violated the schema. Always fail closed."""


def _require_mapping(value, what):
    if not isinstance(value, dict):
        raise SchemaError(f"{what} must be a JSON object")
    return value


def _reject_unknown_fields(mapping, allowed, what):
    unknown = set(mapping) - allowed
    if unknown:
        raise SchemaError(f"{what} has unknown fields: {sorted(unknown)}")


def _require_str(value, what):
    if not isinstance(value, str) or not value:
        raise SchemaError(f"{what} must be a non-empty string")


def _require_schema_version(value, what):
    if value not in SCHEMA_VERSIONS or isinstance(value, bool):
        raise SchemaError(f"{what} must be one of {list(SCHEMA_VERSIONS)}")
    return value


def _validate_v2_params(evaluator, params, path):
    required, optional = EVALUATOR_PARAMS[evaluator]
    params = _require_mapping(params, f"{path}.params")
    _reject_unknown_fields(params, set(required) | set(optional), f"{path}.params")
    for name, kind in {**required, **optional}.items():
        if name not in params:
            if name in required:
                raise SchemaError(f"{path}.params is missing '{name}'")
            continue
        value = params[name]
        if kind == _LIST:
            ok = isinstance(value, list) and all(isinstance(i, str) and i for i in value)
        elif kind == _INT:
            ok = isinstance(value, int) and not isinstance(value, bool) and value >= 0
        else:
            ok = isinstance(value, str) and bool(value)
        if not ok:
            raise SchemaError(f"{path}.params.{name} must be a {kind}")
        if name in FIELD_LIST_PARAMS:
            for field in value:
                if field not in ENVELOPE_FIELDS:
                    raise SchemaError(f"{path}.params.{name} references unknown field '{field}'")
    if evaluator == "evidence_format" and params["expect"] not in EVIDENCE_EXPECTATIONS:
        raise SchemaError(f"{path}.params.expect must be one of {sorted(EVIDENCE_EXPECTATIONS)}")


def validate_condition(condition, path, schema_version=1):
    """Validate one condition node (evaluator or combinator)."""
    condition = _require_mapping(condition, f"{path} condition")
    if "any_of" in condition or "all_of" in condition:
        if len(condition) != 1:
            raise SchemaError(f"{path} combinator must have exactly one any_of/all_of key")
        kind = "any_of" if "any_of" in condition else "all_of"
        children = condition[kind]
        if not isinstance(children, list) or not children:
            raise SchemaError(f"{path}.{kind} must be a non-empty list")
        for index, child in enumerate(children):
            validate_condition(child, f"{path}.{kind}[{index}]", schema_version)
        return
    _reject_unknown_fields(
        condition,
        {"eval", "field", "params", "negate"},
        f"{path} condition",
    )
    evaluator = condition.get("eval")
    _require_str(evaluator, f"{path}.eval")
    if evaluator not in EVALUATORS:
        raise SchemaError(f"{path} references unknown evaluator '{evaluator}'")
    allowed_fields = ENVELOPE_FIELDS if schema_version == 2 else V1_ENVELOPE_FIELDS
    field = condition.get("field")
    if field is not None:
        _require_str(field, f"{path}.field")
        if field not in allowed_fields:
            raise SchemaError(f"{path} references unknown envelope field '{field}'")
    params = condition.get("params")
    if evaluator in EVALUATOR_PARAMS:
        if schema_version != 2:
            raise SchemaError(f"{path} uses '{evaluator}', which needs schema_version 2")
        _validate_v2_params(evaluator, params, path)
    elif params is not None:
        params = _require_mapping(params, f"{path}.params")
    negate = condition.get("negate", False)
    if not isinstance(negate, bool):
        raise SchemaError(f"{path}.negate must be a boolean")


def validate_gate(gate, path, schema_version=1):
    """Validate one gate record."""
    gate = _require_mapping(gate, path)
    v2 = schema_version == 2
    _reject_unknown_fields(gate, V2_GATE_FIELDS if v2 else GATE_FIELDS, path)
    _require_str(gate.get("id"), f"{path}.id")
    if not isinstance(gate.get("hard"), bool):
        raise SchemaError(f"{path}.hard must be a boolean")
    _require_str(gate.get("public_explanation"), f"{path}.public_explanation")
    if v2 and gate.get("category") not in PUBLIC_CATEGORIES:
        raise SchemaError(f"{path}.category must be a public taxonomy category")
    branches = gate.get("branches")
    if not isinstance(branches, list) or not branches:
        raise SchemaError(f"{path}.branches must be a non-empty list")
    statuses = V2_BRANCH_STATUSES if v2 else BRANCH_STATUSES
    for index, branch in enumerate(branches):
        branch_path = f"{path}.branches[{index}]"
        branch = _require_mapping(branch, branch_path)
        _reject_unknown_fields(branch, V2_BRANCH_FIELDS if v2 else BRANCH_FIELDS, branch_path)
        when = branch.get("when")
        if not isinstance(when, list):
            raise SchemaError(f"{branch_path}.when must be a list")
        for cond_index, condition in enumerate(when):
            validate_condition(condition, f"{branch_path}.when[{cond_index}]", schema_version)
        status = branch.get("status")
        _require_str(status, f"{branch_path}.status")
        if status not in statuses:
            raise SchemaError(f"{branch_path}.status must be one of {sorted(statuses)}")
        _require_str(branch.get("reason"), f"{branch_path}.reason")
        if "penalty" in branch:
            penalty = branch["penalty"]
            if (
                isinstance(penalty, bool)
                or not isinstance(penalty, (int, float))
                or not 0 <= penalty <= 1
            ):
                raise SchemaError(f"{branch_path}.penalty must be a number in [0, 1]")
            if status == "Pass" and penalty != 1:
                raise SchemaError(f"{branch_path}: a Pass branch cannot carry a penalty")
    # A final default branch (empty when) is required so evaluation always
    # terminates deterministically.
    if branches[-1].get("when"):
        raise SchemaError(f"{path} must end with a default branch (empty when)")


def validate_migration(migration, path):
    """Validate one migration record."""
    migration = _require_mapping(migration, path)
    _reject_unknown_fields(migration, MIGRATION_FIELDS, path)
    _require_str(migration.get("id"), f"{path}.id")
    family = migration.get("family")
    _require_str(family, f"{path}.family")
    if family not in MIGRATION_FAMILIES:
        raise SchemaError(f"{path}.family must be one of {sorted(MIGRATION_FAMILIES)}")
    gate_ids = migration.get("gate_ids")
    if not isinstance(gate_ids, list) or not all(
        isinstance(gate_id, str) and gate_id for gate_id in gate_ids
    ):
        raise SchemaError(f"{path}.gate_ids must be a list of gate ids")
    _require_str(migration.get("public_explanation"), f"{path}.public_explanation")
    _require_str(migration.get("introduced_version"), f"{path}.introduced_version")


def source_schema_version(document):
    """The schema version a source document declares (default 1)."""
    return _require_schema_version(document.get("schema_version", 1), "source.schema_version")


def validate_source(document):
    """Validate one source document: {schema_version?, label, gates, migrations}."""
    document = _require_mapping(document, "source")
    _reject_unknown_fields(document, SOURCE_FIELDS, "source")
    schema_version = source_schema_version(document)
    _require_str(document.get("label"), "source.label")
    gates = document.get("gates")
    if not isinstance(gates, list) or not gates:
        raise SchemaError("source.gates must be a non-empty list")
    for index, gate in enumerate(gates):
        validate_gate(gate, f"source.gates[{index}]", schema_version)
    migrations = document.get("migrations", [])
    if not isinstance(migrations, list):
        raise SchemaError("source.migrations must be a list")
    for index, migration in enumerate(migrations):
        validate_migration(migration, f"source.migrations[{index}]")
    # Migration records must reference gates that exist in this source.
    known_ids = {gate["id"] for gate in gates}
    for migration in migrations:
        for gate_id in migration["gate_ids"]:
            if gate_id not in known_ids:
                raise SchemaError(
                    f"migration '{migration['id']}' references unknown gate '{gate_id}'"
                )


def validate_artifact(artifact):
    """Validate a compiled ruleset artifact (the committed format)."""
    artifact = _require_mapping(artifact, "artifact")
    _reject_unknown_fields(artifact, ARTIFACT_FIELDS, "artifact")
    schema_version = _require_schema_version(
        artifact.get("schema_version"), "artifact.schema_version"
    )
    _require_str(artifact.get("ruleset_id"), "artifact.ruleset_id")
    _require_str(artifact.get("version"), "artifact.version")
    if not isinstance(artifact.get("created_unix"), int):
        raise SchemaError("artifact.created_unix must be an integer")
    digest = artifact.get("payload_sha256")
    _require_str(digest, "artifact.payload_sha256")
    if len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
        raise SchemaError("artifact.payload_sha256 must be 64 hex characters")
    counts = _require_mapping(artifact.get("counts"), "artifact.counts")
    gates = artifact.get("gates")
    migrations = artifact.get("migrations")
    if not isinstance(gates, list) or not gates:
        raise SchemaError("artifact.gates must be a non-empty list")
    if not isinstance(migrations, list):
        raise SchemaError("artifact.migrations must be a list")
    for index, gate in enumerate(gates):
        validate_gate(gate, f"artifact.gates[{index}]", schema_version)
    for index, migration in enumerate(migrations):
        validate_migration(migration, f"artifact.migrations[{index}]")
    provenance = artifact.get("provenance")
    if not isinstance(provenance, list):
        raise SchemaError("artifact.provenance must be a list")
    for entry in provenance:
        entry = _require_mapping(entry, "artifact.provenance entry")
        _reject_unknown_fields(entry, {"label", "sha256"}, "provenance entry")
        _require_str(entry.get("label"), "provenance.label")
        _require_str(entry.get("sha256"), "provenance.sha256")
    if counts.get("gates") != len(gates) or counts.get("migrations") != len(migrations):
        raise SchemaError("artifact.counts must match the gates and migrations lists")
