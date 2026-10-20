"""Compile knowledge sources into a versioned, opaque ruleset artifact.

Deterministic by construction: the artifact carries created_unix = 0 by
default (overridable for provenance-dated releases), and every field is
serialized canonically (sorted keys, tight separators, ensure_ascii), so
the same sources always produce a byte-identical artifact.
"""

import hashlib
import json
import pathlib

from . import leakage, schema

ARTIFACT_SCHEMA_VERSION = 1


def canonical(value) -> str:
    """The canonical JSON serialization used for all hashing."""
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def sha256_hex(data) -> str:
    if isinstance(data, str):
        data = data.encode("utf-8")
    return hashlib.sha256(data).hexdigest()


def _load_sources(source_dir: pathlib.Path):
    """Load and order the source documents in a directory.

    A manifest.json ({"sources": [...], "precedence": [...]}) orders the
    sources; without one, files are read in sorted name order.
    """
    if not source_dir.is_dir():
        raise schema.SchemaError(f"source directory does not exist: {source_dir}")
    files = sorted(p for p in source_dir.glob("*.json") if p.name != "manifest.json")
    if not files:
        raise schema.SchemaError("source directory contains no source files")
    documents = {}
    for path in files:
        try:
            document = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as err:
            raise schema.SchemaError(f"{path.name} is not valid JSON: {err}") from err
        schema.validate_source(document)
        label = document["label"]
        if label in documents:
            raise schema.SchemaError(f"duplicate source label '{label}'")
        documents[label] = (path, document)

    manifest_path = source_dir / "manifest.json"
    order = list(documents)
    if manifest_path.exists():
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        unknown = set(manifest) - {"sources", "precedence"}
        if unknown:
            raise schema.SchemaError(f"manifest has unknown fields: {sorted(unknown)}")
        precedence = manifest.get("precedence", [])
        if not isinstance(precedence, list):
            raise schema.SchemaError("manifest.precedence must be a list of labels")
        missing = [label for label in precedence if label not in documents]
        if missing:
            raise schema.SchemaError(f"manifest precedence references unknown labels: {missing}")
        unlisted = [label for label in documents if label not in precedence]
        order = list(precedence) + sorted(unlisted)
    return [(label, *documents[label]) for label in order]


def _merge_gates(sources):
    """Merge gates across sources by precedence; identical duplicates
    dedupe, conflicts fail closed."""
    merged = {}
    for label, _path, document in sources:
        for gate in document["gates"]:
            gate_id = gate["id"]
            encoded = canonical(gate)
            if gate_id in merged:
                if merged[gate_id][1] != encoded:
                    raise schema.SchemaError(
                        f"conflicting definitions for gate '{gate_id}' "
                        f"(source '{label}' disagrees with an earlier source)"
                    )
                continue
            merged[gate_id] = (gate, encoded)
    # Insertion order (source order) is preserved: the artifact's gate
    # order is the scanner's execution order and part of the public
    # report contract.
    return [merged[gate_id][0] for gate_id in merged]


def _merge_migrations(sources):
    merged = {}
    for label, _path, document in sources:
        for migration in document.get("migrations", []):
            migration_id = migration["id"]
            encoded = canonical(migration)
            if migration_id in merged:
                if merged[migration_id] != encoded:
                    raise schema.SchemaError(
                        f"conflicting definitions for migration '{migration_id}' "
                        f"(source '{label}' disagrees with an earlier source)"
                    )
                continue
            merged[migration_id] = migration
    return [merged[migration_id] for migration_id in merged]


def compile_ruleset(source_dir, ruleset_id, version, timestamp=0):
    """Compile a source directory into the artifact dict (validated)."""
    sources = _load_sources(pathlib.Path(source_dir))
    gates = _merge_gates(sources)
    migrations = _merge_migrations(sources)

    # Migrations must reference gates that survived the merge.
    known_ids = {gate["id"] for gate in gates}
    for migration in migrations:
        for gate_id in migration["gate_ids"]:
            if gate_id not in known_ids:
                raise schema.SchemaError(
                    f"migration '{migration['id']}' references unknown gate '{gate_id}'"
                )

    provenance = []
    for label, path, _document in sources:
        provenance.append({"label": label, "sha256": sha256_hex(path.read_bytes())})

    findings = leakage.scan_for_leakage(gates, migrations, provenance)
    if findings:
        raise schema.SchemaError(
            "leakage scanner refused to compile: " + leakage.describe_findings(findings)
        )

    artifact = {
        "schema_version": ARTIFACT_SCHEMA_VERSION,
        "ruleset_id": ruleset_id,
        "version": version,
        "created_unix": timestamp,
        "payload_sha256": sha256_hex(canonical(gates)),
        "counts": {"gates": len(gates), "migrations": len(migrations)},
        "provenance": provenance,
        "gates": gates,
        "migrations": migrations,
    }
    schema.validate_artifact(artifact)
    return artifact


def write_artifact(artifact, out_path):
    """Write the artifact in its canonical byte-stable form."""
    out_path = pathlib.Path(out_path)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(canonical(artifact) + "\n", encoding="utf-8")
    return out_path


def read_artifact(path):
    """Load and validate an existing artifact."""
    path = pathlib.Path(path)
    try:
        artifact = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as err:
        raise schema.SchemaError(f"artifact is not valid JSON: {err}") from err
    schema.validate_artifact(artifact)
    return artifact
