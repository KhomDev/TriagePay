"""CI-style hygiene checks over the tracked tree.

- No tracked JSONL file carries a ``hypothesis`` field: benchmark rows are
  read-only inputs and never enter the repository.
- No ``rules-v1.private*`` artifact is tracked: the private ruleset lives
  outside the repository.
- Cassettes hold no auth material.
"""

import json
import pathlib
import subprocess

import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


def tracked_files():
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "ls-files", "-z"], capture_output=True
    )
    if result.returncode != 0:
        pytest.skip("not a git checkout")
    return [name for name in result.stdout.decode("utf-8").split("\0") if name]


def test_no_tracked_jsonl_carries_a_hypothesis_field():
    offenders = []
    for relative in tracked_files():
        if not relative.endswith(".jsonl"):
            continue
        text = (REPO_ROOT / relative).read_text(encoding="utf-8", errors="replace")
        for number, line in enumerate(text.splitlines(), 1):
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(row, dict) and "hypothesis" in row:
                offenders.append(f"{relative}:{number}")
    assert offenders == []


def test_no_private_ruleset_is_tracked():
    assert [f for f in tracked_files() if "rules-v1.private" in f] == []


def test_cassettes_carry_no_auth_material():
    cassettes = REPO_ROOT / "triage-model" / "cassettes"
    for path in cassettes.glob("*.json"):
        text = path.read_text(encoding="utf-8")
        cassette = json.loads(text)
        assert set(cassette) <= {"request_digest", "model_id", "content", "usage"}
        lowered = text.lower()
        assert "authorization" not in lowered and "bearer" not in lowered
