"""CLI bridge v2: engine selection, the triage.v2 sku, and the demo verdict.

The v1 contract (stdin JSON in, report JSON out, exit codes 0/2/1) is
covered by test_cli.py and must keep passing unchanged.
"""

import json
import os
import pathlib
import subprocess
import sys

import pytest

from aps_scanner.__main__ import UsageError, parse_args, run

SCANNER_DIR = pathlib.Path(__file__).resolve().parents[1]
DEMO = SCANNER_DIR.parent / "demo" / "envelope.json"
ADDRESS = "0x5f3b8a1c9d2e47f6a0b1c2d3e4f5a6b7c8d9e0f1"


def run_cli(payload, *args, env=None):
    if not isinstance(payload, (str, bytes)):
        payload = json.dumps(payload)
    if isinstance(payload, str):
        payload = payload.encode("utf-8")
    environment = {
        k: v
        for k, v in os.environ.items()
        if k not in ("APS_LIVE_MODEL", "NEARAI_API_KEY", "APS_TRIAGE_CASSETTES")
    }
    environment.update(env or {})
    return subprocess.run(
        [sys.executable, "-m", "aps_scanner", *args],
        input=payload,
        capture_output=True,
        cwd=SCANNER_DIR,
        env=environment,
        timeout=60,
    )


def v2_envelope(**overrides):
    envelope = {
        "envelope_version": 2,
        "title": "Share inflation lets the first depositor steal later deposits",
        "hypothesis": "A victim depositing 10,000 USDC is minted zero shares and loses 10,000 USDC.",
        "attacker_capability": "Any external address",
        "component_class": "vault",
        "deployed_state_evidence": f"Vault live at {ADDRESS}",
        "reproduction_summary": "Step 1: mint one share. Step 2: donate 10,000 USDC. Step 3: redeem.",
    }
    envelope.update(overrides)
    return envelope


def test_demo_envelope_lands_on_needs_evidence_for_unverified_deployment():
    result = run_cli(DEMO.read_bytes())
    assert result.returncode == 0, result.stderr.decode()
    report = json.loads(result.stdout)
    assert report["report_version"] == 2
    assert report["decision"] == "needs_evidence"
    assert report["confidence_band"] == "medium"
    assert report["taxonomy_hits"] == ["unverified-deployment-evidence"]
    assert report["missing_evidence"] == ["deployment_evidence"]
    gate = next(g for g in report["gate_results"] if g["gate"] == "deployment_evidence")
    assert gate["status"] == "NeedsEvidence"
    assert gate["category"] == "unverified-deployment-evidence"
    assert report["ruleset_version"] == "rules-v1.public"


def test_demo_with_a_well_formed_reference_reaches_supported_stage_a():
    demo = json.loads(DEMO.read_text(encoding="utf-8"))
    demo["deployed_state_evidence"] = f"Vault live at {ADDRESS} since block 19387002"
    report = json.loads(run_cli(demo).stdout)
    assert report["decision"] == "supported"
    assert report["confidence_band"] == "medium"  # Stage A alone is never high


def test_v2_envelope_runs_stage_a_only_by_default():
    report = json.loads(run_cli(v2_envelope()).stdout)
    assert report["stage_b"]["status"] == "not_run"
    assert report["decision"] == "supported"


def test_triage_sku_without_a_model_fails_closed_to_needs_evidence():
    result = run_cli(v2_envelope(), "--sku", "triage.v2", env={"APS_TRIAGE_MODEL_DIR": ""})
    assert result.returncode == 0, result.stderr.decode()
    report = json.loads(result.stdout)
    assert report["stage_b"]["status"] == "unavailable"
    assert report["stage_b"]["failure"] == "model_not_configured"
    assert report["decision"] == "needs_evidence"
    assert report["confidence_band"] == "low"


def test_triage_sku_maps_a_v1_envelope_into_v2():
    v1 = {k: v for k, v in v2_envelope().items() if k not in ("envelope_version", "hypothesis")}
    v1["claimed_impact"] = "Each victim loses up to 10,000 USDC."
    report = json.loads(run_cli(v1, "--sku", "triage.v2").stdout)
    assert report["report_version"] == 2


def test_scan_sku_keeps_v1_envelopes_on_the_v1_engine():
    v1 = {k: v for k, v in v2_envelope().items() if k not in ("envelope_version", "hypothesis")}
    report = json.loads(run_cli(v1, "--sku", "scan.v1").stdout)
    assert "report_version" not in report
    assert report["ruleset_version"] == "rules-v0.synthetic.1"


def test_engine_v1_refuses_v2_input():
    result = run_cli(v2_envelope(), "--engine", "v1")
    assert result.returncode == 2
    assert json.loads(result.stdout)["errors"]


def test_v2_validation_errors_exit_2():
    result = run_cli(v2_envelope(hypothesis="x" * 12001))
    assert result.returncode == 2
    assert any("hypothesis" in e for e in json.loads(result.stdout)["errors"])


def test_bad_command_line_exits_1():
    result = run_cli(v2_envelope(), "--sku", "free.v0")
    assert result.returncode == 1
    assert result.stdout == b""


@pytest.mark.parametrize(
    "argv", [["--sku"], ["--bogus", "x"], ["--engine", "v3"], ["positional"]]
)
def test_parse_args_rejects_bad_input(argv):
    with pytest.raises(UsageError):
        parse_args(argv)


def test_parse_args_defaults():
    assert parse_args([]) == {"sku": "scan.v1", "engine": "auto"}


def test_run_uses_the_injected_judge_loader():
    def loader():
        return lambda _request: {"status": "unavailable", "failure": "model_not_configured"}

    code, payload = run(json.dumps(v2_envelope()), {"sku": "triage.v2", "engine": "auto"}, loader)
    assert code == 0 and payload["stage_b"]["status"] == "unavailable"


def test_v2_cli_is_deterministic():
    first = run_cli(DEMO.read_bytes())
    second = run_cli(DEMO.read_bytes())
    assert first.stdout == second.stdout
