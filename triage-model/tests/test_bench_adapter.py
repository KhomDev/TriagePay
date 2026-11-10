"""bench_adapter smoke tests on synthetic rows (never benchmark data)."""

import json
import pathlib
import subprocess
import sys

import pytest

import bench_adapter
from triage_model.backends import ScriptedBackend
from triage_model.judge import make_judge

ROOT = pathlib.Path(__file__).resolve().parents[1]

TEMPLATES = [
    "A sandwich steals {n}% of every swap because minOut is never checked.",
    "In theory the owner could pause withdrawals.",
    "Rounding lets an attacker drain {n} USDC per call from the buffer.",
    "Requires the admin to set a zero fee; then fees are lost.",
    "No funds are lost, but the event emits the wrong value.",
]


def synthetic_rows(count=20):
    rows = []
    for index in range(count):
        template = TEMPLATES[index % len(TEMPLATES)]
        rows.append(
            {
                "id": f"synthetic-{index:02d}",
                "title": f"Synthetic finding {index}",
                "hypothesis": template.format(n=index + 1),
                "source_dir": "ignored",
            }
        )
    return rows


def write_rows(path, rows):
    path.write_text("".join(json.dumps(r) + "\n" for r in rows), encoding="utf-8")
    return path


def read_rows(path):
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]


def assert_tool_output_format(rows):
    for row in rows:
        assert set(row) <= {"id", "score", "label", "failed"}
        if row.get("failed"):
            assert "score" not in row
        else:
            assert 0 <= row["score"] <= 1
            assert row.get("label", "medium") in ("invalid", "info/low", "medium", "high")


def test_stage_a_smoke_on_20_synthetic_rows(tmp_path):
    rows = synthetic_rows(20)
    out = tmp_path / "out" / "tool_output.jsonl"
    counts = bench_adapter.adapt(write_rows(tmp_path / "in.jsonl", rows), out)
    assert counts == {"rows": 20, "scored": 20, "failed": 0, "labelled": 0}
    produced = read_rows(out)
    assert [r["id"] for r in produced] == [r["id"] for r in rows]
    assert_tool_output_format(produced)
    # The privileged-actor row is penalised below the clean quantified row.
    by_id = {r["id"]: r for r in produced}
    assert by_id["synthetic-03"]["score"] < by_id["synthetic-00"]["score"]


def test_stage_b_smoke_with_a_fake_model(tmp_path):
    def fake(messages):
        return {
            "bountiable_probability": 0.6,
            "severity_call": "medium",
            "taxonomy_hits": [],
            "missing_evidence": [],
            "public_rationale": "Synthetic.",
        }

    judge = make_judge(ScriptedBackend(fake), model_id="scripted/fake")
    out = tmp_path / "tool_output.jsonl"
    counts = bench_adapter.adapt(write_rows(tmp_path / "in.jsonl", synthetic_rows(20)), out, judge)
    assert counts["labelled"] == 20 and counts["failed"] == 0
    produced = read_rows(out)
    assert_tool_output_format(produced)
    assert all(r["label"] == "medium" for r in produced)


def test_fail_closed_stage_b_rows_are_failed_not_zero(tmp_path):
    judge = make_judge(ScriptedBackend(lambda _m: "not json"), model_id="scripted/fake")
    out = tmp_path / "tool_output.jsonl"
    bench_adapter.adapt(write_rows(tmp_path / "in.jsonl", synthetic_rows(3)), out, judge)
    assert read_rows(out) == [{"id": f"synthetic-{i:02d}", "failed": True} for i in range(3)]


def test_bad_rows_become_failed_rows(tmp_path):
    source = tmp_path / "in.jsonl"
    source.write_text(
        '{"id": "ok-1", "title": "t", "hypothesis": "Users lose 5 ETH."}\n'
        "not json\n"
        '{"title": "no id"}\n'
        '{"id": "empty", "title": "", "hypothesis": ""}\n',
        encoding="utf-8",
    )
    out = tmp_path / "out.jsonl"
    counts = bench_adapter.adapt(source, out)
    assert counts["rows"] == 4 and counts["scored"] == 1 and counts["failed"] == 3
    assert read_rows(out)[0]["id"] == "ok-1"


def test_limit_and_overwrite_guard(tmp_path):
    source = write_rows(tmp_path / "in.jsonl", synthetic_rows(10))
    out = tmp_path / "out.jsonl"
    assert bench_adapter.adapt(source, out, limit=4)["rows"] == 4
    with pytest.raises(ValueError):
        bench_adapter.adapt(source, source)


def test_cli_stage_a(tmp_path):
    source = write_rows(tmp_path / "in.jsonl", synthetic_rows(5))
    out = tmp_path / "out.jsonl"
    result = subprocess.run(
        [sys.executable, "bench_adapter.py", "--input", str(source), "--output", str(out)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert result.returncode == 0, result.stderr
    assert "rows 5" in result.stdout
    assert "Synthetic finding" not in result.stdout
    assert len(read_rows(out)) == 5


def test_cli_stage_b_without_a_model_refuses(tmp_path):
    source = write_rows(tmp_path / "in.jsonl", synthetic_rows(2))
    env = {"PATH": "", "SYSTEMROOT": __import__("os").environ.get("SYSTEMROOT", "")}
    result = subprocess.run(
        [sys.executable, "bench_adapter.py", "--input", str(source), "--output", str(tmp_path / "o"), "--stage-b"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=120,
        env=env,
    )
    assert result.returncode == 2
