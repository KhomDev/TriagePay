"""Private ruleset mechanism, exercised with SYNTHETIC private sources only.

No real doctrine is read here: every source is generated in tmp_path
(outside the repository) from the public sample plus synthetic stand-ins.
"""

import copy
import importlib.util
import json
import pathlib
import re
import subprocess
import sys

import pytest

from knowledge_compiler import leakage, private
from knowledge_compiler.compiler import read_artifact

ROOT = pathlib.Path(__file__).resolve().parents[1]
REPO_ROOT = ROOT.parent
PUBLIC_SOURCE = ROOT / "samples" / "public-v1" / "source.json"
STAND_IN = "zz-private-alpha"


def synthetic_private_source(**changes):
    document = json.loads(PUBLIC_SOURCE.read_text(encoding="utf-8"))
    document["label"] = "synthetic-private-doctrine"
    # A private lexicon entry: allowed in params, which never reach reports.
    first_condition = document["gates"][0]["branches"][0]["when"][0]
    first_condition["params"]["context_phrases"].append(f"requires the {STAND_IN} role")
    document.update(changes)
    return document


def write_sources(tmp_path, document):
    directory = tmp_path / "private-sources"
    directory.mkdir(exist_ok=True)
    (directory / "doctrine.json").write_text(json.dumps(document), encoding="utf-8")
    return directory


@pytest.fixture
def stand_in_denylist(monkeypatch, tmp_path):
    path = tmp_path / "private-denylist.sha256"
    path.write_text(leakage.digest_for_term(STAND_IN) + "\n", encoding="utf-8")
    monkeypatch.setenv(leakage.PRIVATE_DENYLIST_ENV, str(path))


def test_private_compile_writes_outside_and_prints_only_the_manifest_hash(
    tmp_path, monkeypatch, capsys, stand_in_denylist
):
    sources = write_sources(tmp_path, synthetic_private_source())
    out = tmp_path / "build" / "rules-v1.private.json"
    monkeypatch.setenv(private.SOURCES_ENV, str(sources))
    monkeypatch.setenv(private.OUT_ENV, str(out))
    assert private.main() == 0
    captured = capsys.readouterr()
    assert re.fullmatch(r"rules-v1\.private manifest sha256 [0-9a-f]{64}\n", captured.out)
    assert str(tmp_path) not in captured.out + captured.err
    artifact = read_artifact(out)
    assert artifact["ruleset_id"] == "rules-v1.private"
    assert captured.out.split()[-1] == private.manifest_sha256(artifact)


def test_private_artifact_is_opaque_and_keeps_public_categories(tmp_path, stand_in_denylist):
    out = tmp_path / "rules-v1.private.json"
    artifact = private.compile_private(write_sources(tmp_path, synthetic_private_source()), out)
    ids = [gate["id"] for gate in artifact["gates"]]
    assert ids == [f"pg-{i:03d}" for i in range(1, len(ids) + 1)]
    assert artifact["migrations"][0]["id"] == "pm-001"
    assert set(artifact["migrations"][0]["gate_ids"]) == set(ids)
    assert artifact["provenance"][0]["label"] == "private-source-01"
    public = json.loads(PUBLIC_SOURCE.read_text(encoding="utf-8"))
    assert [g["category"] for g in artifact["gates"]] == [g["category"] for g in public["gates"]]
    text = out.read_text(encoding="utf-8")
    assert "synthetic-private-doctrine" not in text
    assert "privileged_actor" not in text


def test_private_compile_is_deterministic(tmp_path, stand_in_denylist):
    sources = write_sources(tmp_path, synthetic_private_source())
    first = private.compile_private(sources, tmp_path / "a" / "rules-v1.private.json")
    second = private.compile_private(sources, tmp_path / "b" / "rules-v1.private.json")
    assert private.manifest_sha256(first) == private.manifest_sha256(second)


def test_scanner_loads_and_runs_the_private_artifact(tmp_path, stand_in_denylist):
    out = tmp_path / "rules-v1.private.json"
    private.compile_private(write_sources(tmp_path, synthetic_private_source()), out)
    scanner_dir = REPO_ROOT / "scanner"
    code = (
        "import json, sys\n"
        "from aps_scanner.ruleset import default_v2_ruleset\n"
        "from aps_scanner.envelope_v2 import FindingEnvelopeV2\n"
        "from aps_scanner.engine_v2 import triage\n"
        "ruleset = default_v2_ruleset()\n"
        "env = FindingEnvelopeV2(title='t', hypothesis='Requires the zz-private-alpha role "
        "to drain 5 ETH.', attacker_capability='')\n"
        "print(json.dumps(triage(env, ruleset=ruleset).to_public_json()))\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", code],
        cwd=scanner_dir,
        capture_output=True,
        text=True,
        timeout=60,
        env={**__import__("os").environ, "APS_RULESET_V2_PATH": str(out)},
    )
    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["ruleset_version"] == "rules-v1.private"
    # The private lexicon fired, but the report names only an opaque gate
    # and a public category, never the lexicon text.
    assert "privileged-actor-required" in report["taxonomy_hits"]
    assert "pg-001" in report["failed_gates"]
    assert STAND_IN not in json.dumps(report)


def test_report_facing_leak_is_refused(tmp_path, stand_in_denylist):
    document = synthetic_private_source()
    document["gates"][0]["branches"][0]["reason"] = f"matches the {STAND_IN} pattern"
    with pytest.raises(private.PrivateCompileError, match="report-facing"):
        private.compile_private(write_sources(tmp_path, document), tmp_path / "o.json")


def test_structural_leak_in_params_is_refused(tmp_path, stand_in_denylist):
    document = synthetic_private_source()
    params = document["gates"][0]["branches"][0]["when"][0]["params"]
    params["context_phrases"].append("see " + leakage.HOST_PATHS[-1] + "someone/notes")
    with pytest.raises(private.PrivateCompileError, match="structural"):
        private.compile_private(write_sources(tmp_path, document), tmp_path / "o.json")


def test_sources_inside_the_repository_are_refused(tmp_path):
    with pytest.raises(private.PrivateCompileError, match="outside the repository"):
        private.compile_private(ROOT / "samples" / "public-v1", tmp_path / "o.json")


def test_output_inside_the_repository_is_refused(tmp_path):
    sources = write_sources(tmp_path, synthetic_private_source())
    with pytest.raises(private.PrivateCompileError, match="outside the repository"):
        private.compile_private(sources, REPO_ROOT / "scanner" / "rulesets" / "rules-v1.private.json")
    assert not (REPO_ROOT / "scanner" / "rulesets" / "rules-v1.private.json").exists()


def test_schema_1_sources_are_refused(tmp_path):
    synthetic = json.loads((ROOT / "samples" / "synthetic" / "source.json").read_text(encoding="utf-8"))
    with pytest.raises(private.PrivateCompileError, match="schema_version 2"):
        private.compile_private(write_sources(tmp_path, synthetic), tmp_path / "o.json")


def test_missing_configuration_exits_2(capsys):
    assert private.main(environ={}) == 2
    assert private.main(environ={private.SOURCES_ENV: "x"}) == 2


def test_cli_entry_point(tmp_path, stand_in_denylist):
    sources = write_sources(tmp_path, copy.deepcopy(synthetic_private_source()))
    out = tmp_path / "cli" / "rules-v1.private.json"
    result = subprocess.run(
        [sys.executable, "-m", "knowledge_compiler", "compile-private", "--out", str(out)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=60,
        env={**__import__("os").environ, private.SOURCES_ENV: str(sources)},
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.startswith("rules-v1.private manifest sha256 ")
    assert out.exists()


def test_no_private_ruleset_is_tracked():
    listing = subprocess.run(["git", "-C", str(REPO_ROOT), "ls-files"], capture_output=True, text=True)
    if listing.returncode != 0:
        pytest.skip("not a git checkout")
    assert [name for name in listing.stdout.splitlines() if "rules-v1.private" in name] == []


def test_private_module_reads_no_real_doctrine_by_default():
    # The module has no built-in source path: without APS_PRIVATE_SOURCES it
    # refuses to run instead of falling back to any location.
    spec = importlib.util.find_spec("knowledge_compiler.private")
    text = pathlib.Path(spec.origin).read_text(encoding="utf-8")
    assert "Desktop" not in text and "doctrine/" not in text
