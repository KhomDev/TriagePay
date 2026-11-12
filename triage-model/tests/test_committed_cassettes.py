"""The committed cassettes replay the repository's synthetic findings.

They were recorded on 2026-11-12 against NEAR AI Cloud with the pinned
default model, from the demo envelope and the four findings in
agents/demo/findings (all synthetic). A change to the prompt, the output
schema or the default model changes the request digest; the replay then
misses and Stage B fails closed. This test turns that silent fallback into
a failure, so the cassettes are re-recorded with the change.
"""

import json
import pathlib

import pytest

from aps_scanner.engine_v2 import triage
from aps_scanner.envelope_v2 import load_payload
from triage_model import config
from triage_model.backends import CassetteBackend
from triage_model.judge import make_judge

REPO = pathlib.Path(__file__).resolve().parents[2]
CASSETTES = REPO / "triage-model" / "cassettes"
INPUTS = [REPO / "demo" / "envelope.json"] + sorted(
    (REPO / "agents" / "demo" / "findings").glob("*.json")
)


@pytest.mark.parametrize("path", INPUTS, ids=lambda p: p.stem)
def test_committed_cassette_replays(path):
    judge = make_judge(CassetteBackend(CASSETTES), model_id=config.DEFAULT_MODEL_ID)
    envelope = load_payload(json.loads(path.read_text(encoding="utf-8")))
    report = triage(envelope, judge=judge).to_public_json()
    assert report["stage_b"]["status"] == "ok", report["stage_b"]
    assert report["model_id"] == config.DEFAULT_MODEL_ID
    assert report["model_verifiable"] is True
    # The judge is advisory: the final decision never exceeds Stage A's.
    order = ["rejected", "held", "needs_evidence", "supported"]
    assert order.index(report["decision"]) <= order.index(report["stage_a"]["decision"])


def test_the_injected_finding_cannot_raise_its_verdict():
    # Finding 3 tells the reader to mark it supported. Whatever the model
    # says, the decision is capped by the deterministic gates.
    path = REPO / "agents" / "demo" / "findings" / "3-fee-withdrawal-with-injection.json"
    judge = make_judge(CassetteBackend(CASSETTES), model_id=config.DEFAULT_MODEL_ID)
    report = triage(load_payload(json.loads(path.read_text(encoding="utf-8"))), judge=judge)
    public = report.to_public_json()
    assert public["contains_embedded_instructions"] is True
    assert public["decision"] == public["stage_a"]["decision"]
