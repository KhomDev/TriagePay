"""Shared synthetic v2 envelopes and scripted Stage B judges for tests."""

import hashlib

from aps_scanner.envelope_v2 import FindingEnvelopeV2

ADDRESS = "0x5f3b8a1c9d2e47f6a0b1c2d3e4f5a6b7c8d9e0f1"
PROMPT_HASH = hashlib.sha256(b"scripted-prompt").hexdigest()
MODEL_ID = "scripted/fake-model"


def strong(**overrides):
    fields = dict(
        title="Share inflation lets the first depositor steal later deposits",
        hypothesis=(
            "The first depositor mints one share, donates 10,000 USDC to the vault, and "
            "every later depositor of up to 10,000 USDC receives zero shares, so the "
            "attacker redeems their deposits."
        ),
        claimed_impact="Each victim loses up to 10,000 USDC of deposited funds.",
        mechanism="Share price rounding with an empty vault",
        attacker_capability="Any external address with collateral",
        preconditions="Vault freshly deployed with empty reserves",
        component_class="vault",
        scope_statement="vault deposit and mint path",
        deployed_state_evidence=f"Vault live at {ADDRESS} since block 19387002",
        reproduction_summary="Step 1: mint one share. Step 2: donate 10,000 USDC. Step 3: redeem.",
    )
    fields.update(overrides)
    return FindingEnvelopeV2(**fields)


# One envelope per Stage A decision.
STAGE_A_FIXTURES = {
    "supported": {},
    "needs_evidence": {"deployed_state_evidence": "Vault live at 0x1234567890abcdef"},
    "held": {"known_issue_declaration": "listed in the known-issues section"},
    "rejected": {"attacker_capability": "The protocol owner"},
}


def verdict(probability, **overrides):
    result = {
        "status": "ok",
        "bountiable_probability": probability,
        "severity_call": "high" if probability > 0.65 else "invalid",
        "taxonomy_hits": [],
        "missing_evidence": [],
        "public_rationale": "Generic rationale for a synthetic finding.",
        "response_digest": hashlib.sha256(str(probability).encode()).hexdigest(),
        "model_id": MODEL_ID,
        "prompt_hash": PROMPT_HASH,
    }
    result.update(overrides)
    return result


def judge_returning(result):
    def judge(request):
        judge.requests.append(request)
        return result

    judge.requests = []
    return judge
