"""Private ruleset compile: rules-v1.private, built and kept outside the repository.

The operator's doctrine sources are read from the directory named by
APS_PRIVATE_SOURCES (read only, never copied). The artifact is written to
--out or APS_PRIVATE_RULESET_OUT. Both locations must lie outside this
repository, and the command prints only the manifest hash.

What the artifact may and may not carry:

- gate ids, migration ids and provenance labels are replaced by opaque
  positional ids (pg-001, pm-001, private-source-01), so source naming
  never reaches a report;
- every gate keeps one public taxonomy category, so reports produced with
  it speak only the public vocabulary;
- everything that can reach a report (reasons, explanations, ids,
  categories) passes the full leakage scan, denylist included;
- evaluator params (the private lexicons) never reach a report and are
  scanned only for structural leaks (host paths, key material, secret
  shapes); they may contain denylisted vocabulary because they live off-repo.
"""

import os
import pathlib
import subprocess
import sys

from . import compiler, leakage, schema

SOURCES_ENV = "APS_PRIVATE_SOURCES"
OUT_ENV = "APS_PRIVATE_RULESET_OUT"
RULESET_ID = "rules-v1.private"
VERSION = "1.0.0"

PACKAGE_ROOT = pathlib.Path(__file__).resolve().parent.parent


class PrivateCompileError(ValueError):
    """The private compile refused to run (messages never carry paths)."""


def repo_root() -> pathlib.Path:
    result = subprocess.run(
        ["git", "-C", str(PACKAGE_ROOT), "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0 and result.stdout.strip():
        return pathlib.Path(result.stdout.strip()).resolve()
    return PACKAGE_ROOT.parent.resolve()


def _inside(path: pathlib.Path, root: pathlib.Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


def _strip_params(node):
    """A deep copy of a gate structure without evaluator params."""
    if isinstance(node, dict):
        return {k: _strip_params(v) for k, v in node.items() if k != "params"}
    if isinstance(node, list):
        return [_strip_params(item) for item in node]
    return node


def _params_of(node, found):
    if isinstance(node, dict):
        if "params" in node:
            found.append(node["params"])
        for value in node.values():
            _params_of(value, found)
    elif isinstance(node, list):
        for item in node:
            _params_of(item, found)
    return found


def _structural_leaks(text):
    """Host paths and key or secret shapes (no denylist, no URL policy)."""
    hits = [f"host path '{p}'" for p in leakage.HOST_PATHS if p in text]
    hits += [f"key prefix '{p}'" for p in leakage.KEY_PREFIXES if p in text]
    if leakage.BASE58_RUN_PATTERN.search(text):
        hits.append("long base58 run")
    if leakage.HEX_RUN_PATTERN.search(text):
        hits.append("long 0x-hex run")
    return hits


def manifest(artifact) -> dict:
    return {
        "ruleset_id": artifact["ruleset_id"],
        "version": artifact["version"],
        "schema_version": artifact["schema_version"],
        "payload_sha256": artifact["payload_sha256"],
        "counts": artifact["counts"],
    }


def manifest_sha256(artifact) -> str:
    return compiler.sha256_hex(compiler.canonical(manifest(artifact)))


def compile_private(source_dir, out_path, root=None, environ=None):
    """Compile, check, and write the private artifact; return it.

    The private denylist (APS_PRIVATE_DENYLIST) is mandatory here: this is
    the one compile whose report-facing text must be checked against the
    real terms, so it fails closed instead of falling back to the published
    synthetic stand-ins.
    """
    environ = os.environ if environ is None else environ
    try:
        private_terms = leakage.load_private_denylist(environ)
    except leakage.DenylistError as err:
        raise PrivateCompileError(str(err)) from err
    if not private_terms:
        raise PrivateCompileError(
            f"{leakage.PRIVATE_DENYLIST_ENV} must name a non-empty file of real-term digests; "
            "compile-private does not fall back to the published stand-ins"
        )
    root = repo_root() if root is None else pathlib.Path(root)
    source_dir = pathlib.Path(source_dir)
    out_path = pathlib.Path(out_path)
    if _inside(source_dir, root):
        raise PrivateCompileError("the private sources must live outside the repository")
    if _inside(out_path, root):
        raise PrivateCompileError("the private ruleset must be written outside the repository")

    sources = compiler._load_sources(source_dir)
    if compiler._sources_schema_version(sources) != 2:
        raise PrivateCompileError("private sources must declare schema_version 2")
    gates = compiler._merge_gates(sources)
    migrations = compiler._merge_migrations(sources)

    id_map = {gate["id"]: f"pg-{index:03d}" for index, gate in enumerate(gates, 1)}
    gates = [dict(gate, id=id_map[gate["id"]]) for gate in gates]
    opaque_migrations = []
    for index, migration in enumerate(migrations, 1):
        missing = [g for g in migration["gate_ids"] if g not in id_map]
        if missing:
            raise PrivateCompileError("a migration references an unknown gate")
        opaque_migrations.append(
            dict(
                migration,
                id=f"pm-{index:03d}",
                gate_ids=[id_map[g] for g in migration["gate_ids"]],
            )
        )
    provenance = [
        {
            "label": f"private-source-{index:02d}",
            "sha256": compiler.sha256_hex(compiler.canonical(document)),
        }
        for index, (_label, _path, document) in enumerate(sources, 1)
    ]

    findings = leakage.scan_for_leakage(_strip_params(gates), opaque_migrations, provenance)
    if findings:
        raise PrivateCompileError(
            "leakage scanner refused the report-facing fields: "
            + leakage.describe_findings(findings)
        )
    params_text = compiler.canonical(_params_of(gates, []))
    structural = _structural_leaks(params_text)
    if structural:
        raise PrivateCompileError(
            "structural leak in evaluator params: " + "; ".join(sorted(set(structural)))
        )

    artifact = {
        "schema_version": 2,
        "ruleset_id": RULESET_ID,
        "version": VERSION,
        "created_unix": 0,
        "payload_sha256": compiler.sha256_hex(compiler.canonical(gates)),
        "counts": {"gates": len(gates), "migrations": len(opaque_migrations)},
        "provenance": provenance,
        "gates": gates,
        "migrations": opaque_migrations,
    }
    schema.validate_artifact(artifact)
    compiler.write_artifact(artifact, out_path)
    return artifact


def main(out_arg=None, environ=None):
    environ = os.environ if environ is None else environ
    source = environ.get(SOURCES_ENV, "").strip()
    out = (out_arg or environ.get(OUT_ENV, "")).strip()
    if not source:
        print(f"compile-private: {SOURCES_ENV} is not set", file=sys.stderr)
        return 2
    if not out:
        print(f"compile-private: pass --out or set {OUT_ENV}", file=sys.stderr)
        return 2
    try:
        artifact = compile_private(source, out, environ=environ)
    except ValueError as err:  # PrivateCompileError, SchemaError
        print(f"compile-private refused: {err}", file=sys.stderr)
        return 2
    print(f"{RULESET_ID} manifest sha256 {manifest_sha256(artifact)}")
    return 0
