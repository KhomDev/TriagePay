"""rules-v1.public: schema 2 compile, byte stability, and fail-closed checks."""

import copy
import json
import pathlib
import subprocess
import sys

import pytest

from knowledge_compiler.compiler import compile_ruleset, write_artifact
from knowledge_compiler.schema import PUBLIC_CATEGORIES, SchemaError

ROOT = pathlib.Path(__file__).resolve().parents[1]
PUBLIC = ROOT / "samples" / "public-v1"
COMMITTED = ROOT.parent / "scanner" / "rulesets" / "rules-v1.public.json"


def source():
    return json.loads((PUBLIC / "source.json").read_text(encoding="utf-8"))


def compile_doc(tmp_path, document):
    directory = tmp_path / "src"
    directory.mkdir(exist_ok=True)
    (directory / "source.json").write_text(json.dumps(document), encoding="utf-8")
    return compile_ruleset(directory, "rules-v1.public", "1.0.0")


def test_public_ruleset_compiles_as_schema_2():
    artifact = compile_ruleset(PUBLIC, "rules-v1.public", "1.0.0")
    assert artifact["schema_version"] == 2
    assert artifact["counts"] == {"gates": 9, "migrations": 1}
    assert {gate["category"] for gate in artifact["gates"]} == set(PUBLIC_CATEGORIES)


def test_public_ruleset_matches_the_committed_bytes(tmp_path):
    artifact = compile_ruleset(PUBLIC, "rules-v1.public", "1.0.0")
    written = write_artifact(artifact, tmp_path / "out.json").read_bytes()
    assert b"\r" not in written
    assert written == COMMITTED.read_bytes()


def test_check_without_arguments_checks_every_committed_build():
    result = subprocess.run(
        [sys.executable, "-m", "knowledge_compiler", "compile", "--check"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert result.returncode == 0, result.stderr
    assert "rules-v0.synthetic.1.json" in result.stdout
    assert "rules-v1.public.json" in result.stdout


def test_missing_category_fails_closed(tmp_path):
    document = source()
    del document["gates"][0]["category"]
    with pytest.raises(SchemaError, match="category"):
        compile_doc(tmp_path, document)


def test_private_category_fails_closed(tmp_path):
    document = source()
    document["gates"][0]["category"] = "internal-only-bucket"
    with pytest.raises(SchemaError, match="category"):
        compile_doc(tmp_path, document)


def test_pass_branch_cannot_carry_a_penalty(tmp_path):
    document = source()
    document["gates"][0]["branches"][-1]["penalty"] = 0.5
    with pytest.raises(SchemaError, match="Pass branch"):
        compile_doc(tmp_path, document)


def test_v2_evaluator_needs_schema_2(tmp_path):
    document = copy.deepcopy(source())
    document["schema_version"] = 1
    for gate in document["gates"]:
        del gate["category"]
        for branch in gate["branches"]:
            branch.pop("penalty", None)
    with pytest.raises(SchemaError):
        compile_doc(tmp_path, document)


def test_unknown_field_in_params_fails_closed(tmp_path):
    document = source()
    document["gates"][0]["branches"][0]["when"][0]["params"]["capability_fields"] = ["nope"]
    with pytest.raises(SchemaError, match="unknown field"):
        compile_doc(tmp_path, document)


def test_mixed_schema_versions_fail_closed(tmp_path):
    directory = tmp_path / "mixed"
    directory.mkdir()
    (directory / "a.json").write_text(json.dumps(source()), encoding="utf-8")
    synthetic = json.loads(
        (ROOT / "samples" / "synthetic" / "source.json").read_text(encoding="utf-8")
    )
    (directory / "b.json").write_text(json.dumps(synthetic), encoding="utf-8")
    with pytest.raises(SchemaError, match="schema_version"):
        compile_ruleset(directory, "x", "1.0.0")


def test_held_needs_schema_2(tmp_path):
    synthetic = json.loads(
        (ROOT / "samples" / "synthetic" / "source.json").read_text(encoding="utf-8")
    )
    synthetic["gates"][0]["branches"][0]["status"] = "Held"
    with pytest.raises(SchemaError, match="status"):
        compile_doc(tmp_path, synthetic)
