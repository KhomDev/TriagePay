"""Compiler tests: determinism, structure, fail-closed conflicts, mutation."""

import json
import pathlib
import subprocess
import sys

import pytest

from knowledge_compiler.compiler import compile_ruleset, read_artifact, write_artifact
from knowledge_compiler.schema import SchemaError

SAMPLE = pathlib.Path(__file__).resolve().parent.parent / "samples" / "synthetic"


def compile_sample(tmp_path, **kwargs):
    artifact = compile_ruleset(SAMPLE, "rules-v0.synthetic.1", "0.1.0")
    return artifact, write_artifact(artifact, tmp_path / "ruleset.json")


def test_artifact_structure_and_counts():
    artifact = compile_ruleset(SAMPLE, "rules-v0.synthetic.1", "0.1.0")
    assert artifact["schema_version"] == 1
    assert artifact["ruleset_id"] == "rules-v0.synthetic.1"
    assert artifact["counts"] == {"gates": 9, "migrations": 4}
    assert len(artifact["provenance"]) == 1
    assert artifact["provenance"][0]["label"] == "synthetic-sample"
    assert len(artifact["provenance"][0]["sha256"]) == 64
    gate_ids = [gate["id"] for gate in artifact["gates"]]
    # Source order is preserved (execution order is part of the contract).
    assert gate_ids[0] == "scope_statement_present"
    assert gate_ids[-1] == "no_loss_concession"
    assert len(gate_ids) == 9


def test_compile_is_byte_deterministic(tmp_path):
    first, first_path = compile_sample(tmp_path)
    second = compile_ruleset(SAMPLE, "rules-v0.synthetic.1", "0.1.0")
    assert first_path.read_bytes() == (write_artifact(second, tmp_path / "again.json")).read_bytes()
    assert first["payload_sha256"] == second["payload_sha256"]


def test_written_artifact_is_lf_and_matches_the_committed_bytes(tmp_path):
    # Text mode would write CRLF on Windows; the artifact must be LF on
    # every platform so --check is byte-stable everywhere.
    _artifact, path = compile_sample(tmp_path)
    written = path.read_bytes()
    assert b"\r" not in written
    committed = SAMPLE.parents[2] / "scanner" / "rulesets" / "rules-v0.synthetic.1.json"
    assert written == committed.read_bytes()


def test_read_artifact_round_trips(tmp_path):
    _artifact, path = compile_sample(tmp_path)
    loaded = read_artifact(path)
    assert loaded["counts"]["gates"] == 9


def test_removing_a_gate_changes_the_hash(tmp_path, tmp_path_factory):
    source = json.loads((SAMPLE / "source.json").read_text(encoding="utf-8"))
    baseline = compile_ruleset(SAMPLE, "rules-v0.synthetic.1", "0.1.0")
    mutated_dir = tmp_path / "mutated"
    mutated_dir.mkdir()
    for gate_id in [gate["id"] for gate in source["gates"]]:
        mutated = {
            "label": "synthetic-sample",
            "gates": [g for g in source["gates"] if g["id"] != gate_id],
            "migrations": [m for m in source["migrations"] if gate_id not in m["gate_ids"]],
        }
        (mutated_dir / "source.json").write_text(json.dumps(mutated), encoding="utf-8")
        artifact = compile_ruleset(mutated_dir, "rules-v0.synthetic.1", "0.1.0")
        assert artifact["payload_sha256"] != baseline["payload_sha256"], gate_id


def _write_source(tmp_path, document):
    directory = tmp_path / "src"
    directory.mkdir(exist_ok=True)
    (directory / "source.json").write_text(json.dumps(document), encoding="utf-8")
    return directory


def _base_gate(gate_id="impact_sink"):
    return {
        "id": gate_id,
        "hard": True,
        "public_explanation": "x",
        "branches": [{"when": [], "status": "Pass", "reason": "ok"}],
    }


def test_unknown_evaluator_fails_closed(tmp_path):
    gate = _base_gate()
    gate["branches"][0]["when"] = [{"eval": "not_an_evaluator", "field": "title"}]
    directory = _write_source(tmp_path, {"label": "s", "gates": [gate]})
    with pytest.raises(SchemaError, match="unknown evaluator"):
        compile_ruleset(directory, "r", "0.1.0")


def test_unknown_gate_field_fails_closed(tmp_path):
    gate = _base_gate()
    gate["secret_note"] = "anything"
    directory = _write_source(tmp_path, {"label": "s", "gates": [gate]})
    with pytest.raises(SchemaError, match="unknown fields"):
        compile_ruleset(directory, "r", "0.1.0")


def test_empty_gates_fail_closed(tmp_path):
    directory = _write_source(tmp_path, {"label": "s", "gates": []})
    with pytest.raises(SchemaError, match="non-empty"):
        compile_ruleset(directory, "r", "0.1.0")


def test_migration_to_unknown_gate_fails_closed(tmp_path):
    migration = {
        "id": "m",
        "family": "realism",
        "gate_ids": ["missing_gate"],
        "public_explanation": "x",
        "introduced_version": "0.1.0",
    }
    directory = _write_source(
        tmp_path, {"label": "s", "gates": [_base_gate()], "migrations": [migration]}
    )
    with pytest.raises(SchemaError, match="unknown gate"):
        compile_ruleset(directory, "r", "0.1.0")


def test_conflicting_duplicate_gate_fails_closed(tmp_path):
    gate_a = _base_gate("impact_sink")
    gate_b = _base_gate("impact_sink")
    gate_b["branches"][0]["reason"] = "different reason"
    directory = tmp_path / "src"
    directory.mkdir()
    (directory / "a.json").write_text(
        json.dumps({"label": "a", "gates": [gate_a]}), encoding="utf-8"
    )
    (directory / "b.json").write_text(
        json.dumps({"label": "b", "gates": [gate_b]}), encoding="utf-8"
    )
    with pytest.raises(SchemaError, match="conflicting"):
        compile_ruleset(directory, "r", "0.1.0")


def test_identical_duplicate_gate_dedupes(tmp_path):
    gate = _base_gate("impact_sink")
    directory = tmp_path / "src"
    directory.mkdir()
    (directory / "a.json").write_text(json.dumps({"label": "a", "gates": [gate]}), encoding="utf-8")
    (directory / "b.json").write_text(
        json.dumps({"label": "b", "gates": [dict(gate)]}), encoding="utf-8"
    )
    artifact = compile_ruleset(directory, "r", "0.1.0")
    assert artifact["counts"]["gates"] == 1


def test_cli_check_passes_for_the_committed_artifact(tmp_path):
    out = tmp_path / "ruleset.json"
    artifact = compile_ruleset(SAMPLE, "rules-v0.synthetic.1", "0.1.0")
    write_artifact(artifact, out)
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "knowledge_compiler",
            "compile",
            "--source",
            str(SAMPLE),
            "--out",
            str(out),
            "--check",
        ],
        cwd=pathlib.Path(__file__).resolve().parent.parent,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr


def test_cli_check_fails_on_a_tampered_artifact(tmp_path):
    out = tmp_path / "ruleset.json"
    artifact = compile_ruleset(SAMPLE, "rules-v0.synthetic.1", "0.1.0")
    write_artifact(artifact, out)
    tampered = dict(artifact)
    tampered["version"] = "9.9.9"
    write_artifact(tampered, out)
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "knowledge_compiler",
            "compile",
            "--source",
            str(SAMPLE),
            "--out",
            str(out),
            "--check",
        ],
        cwd=pathlib.Path(__file__).resolve().parent.parent,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 2, result.stdout
