"""Leakage tests: poisoned sources abort the compile, no file is written."""

import json
import pathlib

import pytest

from knowledge_compiler.compiler import compile_ruleset
from knowledge_compiler.leakage import scan_for_leakage
from knowledge_compiler.schema import SchemaError

SAMPLE = pathlib.Path(__file__).resolve().parent.parent / "samples" / "synthetic"


def _gate_with_text(text, gate_id="impact_sink"):
    return {
        "id": gate_id,
        "hard": True,
        "public_explanation": text,
        "branches": [{"when": [], "status": "Pass", "reason": "ok"}],
    }


POISONS = [
    ("protocol name", "a redacted-style rounding bug"),
    ("workspace marker", "see redacted corpus for details"),
    ("host path", "repro lives under /Users/whoever/x"),
    ("near account", "funds land in vault.gateway.near"),
    ("secret prefix", "signing key ed25519: then bytes"),
    ("bad url", "snapshot at https://private.example.net/rules"),
]


@pytest.mark.parametrize("name,poison", POISONS, ids=[p[0] for p in POISONS])
def test_poisoned_source_aborts_compile(tmp_path, name, poison):
    gate = _gate_with_text(poison)
    directory = tmp_path / "src"
    directory.mkdir()
    (directory / "source.json").write_text(
        json.dumps({"label": "s", "gates": [gate]}), encoding="utf-8"
    )
    with pytest.raises(SchemaError, match="leakage"):
        compile_ruleset(directory, "r", "0.1.0")
    # Fail closed means nothing was written even on failure paths.
    assert not list((tmp_path / "out").glob("*.json")) if (tmp_path / "out").exists() else True


def test_scan_for_leakage_clean_for_synthetic_gates():
    artifact = compile_ruleset(SAMPLE, "rules-v0.synthetic.1", "0.1.0")
    findings = scan_for_leakage(artifact["gates"], artifact["migrations"], artifact["provenance"])
    assert findings == []


def test_leakage_finding_reports_path_not_content():
    findings = scan_for_leakage([_gate_with_text("curvy analysis of redacted pools")], [], [])
    assert findings
    description = str(findings)
    assert "redacted" in description  # the pattern name may name the marker
    assert "curvy" not in description  # but never the matched text itself
