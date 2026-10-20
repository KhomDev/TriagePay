"""Schema validation for knowledge sources and compiled artifacts.

Fail closed: any unknown field, unknown evaluator id, illegal branch
status, or malformed structure is a SchemaError, never a silent default.
"""

# The fixed evaluator registry. aps-scanner implements exactly these
# evaluators; a ruleset referencing anything else is rejected at load
# time (fail closed) rather than default-passed.
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

BRANCH_STATUSES = frozenset({"Pass", "Fail", "NeedsEvidence"})

MIGRATION_FAMILIES = frozenset(
    {
        "realism",
        "deployed_state",
        "public_evidence",
        "adversarial_review",
    }
)

# Envelope fields the evaluators may reference. Kept in sync with
# aps_scanner.schema.FindingEnvelope.
ENVELOPE_FIELDS = frozenset(
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

GATE_FIELDS = frozenset({"id", "hard", "public_explanation", "branches"})
BRANCH_FIELDS = frozenset({"when", "status", "reason"})
MIGRATION_FIELDS = frozenset(
    {"id", "family", "gate_ids", "public_explanation", "introduced_version"}
)
SOURCE_FIELDS = frozenset({"label", "gates", "migrations"})
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


def validate_condition(condition, path):
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
            validate_condition(child, f"{path}.{kind}[{index}]")
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
    field = condition.get("field")
    if field is not None:
        _require_str(field, f"{path}.field")
        if field not in ENVELOPE_FIELDS:
            raise SchemaError(f"{path} references unknown envelope field '{field}'")
    params = condition.get("params")
    if params is not None:
        params = _require_mapping(params, f"{path}.params")
    negate = condition.get("negate", False)
    if not isinstance(negate, bool):
        raise SchemaError(f"{path}.negate must be a boolean")


def validate_gate(gate, path):
    """Validate one gate record."""
    gate = _require_mapping(gate, path)
    _reject_unknown_fields(gate, GATE_FIELDS, path)
    _require_str(gate.get("id"), f"{path}.id")
    if not isinstance(gate.get("hard"), bool):
        raise SchemaError(f"{path}.hard must be a boolean")
    _require_str(gate.get("public_explanation"), f"{path}.public_explanation")
    branches = gate.get("branches")
    if not isinstance(branches, list) or not branches:
        raise SchemaError(f"{path}.branches must be a non-empty list")
    for index, branch in enumerate(branches):
        branch_path = f"{path}.branches[{index}]"
        branch = _require_mapping(branch, branch_path)
        _reject_unknown_fields(branch, BRANCH_FIELDS, branch_path)
        when = branch.get("when")
        if not isinstance(when, list):
            raise SchemaError(f"{branch_path}.when must be a list")
        for cond_index, condition in enumerate(when):
            validate_condition(condition, f"{branch_path}.when[{cond_index}]")
        status = branch.get("status")
        _require_str(status, f"{branch_path}.status")
        if status not in BRANCH_STATUSES:
            raise SchemaError(f"{branch_path}.status must be one of {sorted(BRANCH_STATUSES)}")
        _require_str(branch.get("reason"), f"{branch_path}.reason")
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


def validate_source(document):
    """Validate one source document: {label, gates, migrations}."""
    document = _require_mapping(document, "source")
    _reject_unknown_fields(document, SOURCE_FIELDS, "source")
    _require_str(document.get("label"), "source.label")
    gates = document.get("gates")
    if not isinstance(gates, list) or not gates:
        raise SchemaError("source.gates must be a non-empty list")
    for index, gate in enumerate(gates):
        validate_gate(gate, f"source.gates[{index}]")
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
    if artifact.get("schema_version") != 1:
        raise SchemaError("artifact.schema_version must be 1")
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
        validate_gate(gate, f"artifact.gates[{index}]")
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
