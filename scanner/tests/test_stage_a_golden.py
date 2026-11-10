"""Stage A golden decisions over synthetic v2 envelopes (rules-v1.public).

Every public taxonomy category is hit by at least one case; the clean
baseline proves the gates also pass.
"""

import pytest

from aps_scanner.envelope_v2 import FindingEnvelopeV2, from_benchmark_row
from aps_scanner.normalize import normalize_envelope_v2
from aps_scanner.ruleset import load_bundled_ruleset
from aps_scanner.stage_a import run_stage_a
from aps_scanner.taxonomy import PUBLIC_CATEGORIES

ADDRESS = "0x5f3b8a1c9d2e47f6a0b1c2d3e4f5a6b7c8d9e0f1"


def base(**overrides):
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


def stage_a(envelope):
    assert envelope.validate() == []
    return run_stage_a(normalize_envelope_v2(envelope))


CASES = [
    # (case id, overrides, expected decision, expected taxonomy hits)
    ("clean", {}, "supported", []),
    (
        "privileged-capability",
        {"attacker_capability": "The protocol owner"},
        "rejected",
        ["privileged-actor-required"],
    ),
    (
        "privileged-context",
        {
            "attacker_capability": "",
            "preconditions": "The exploit requires the admin to set a zero fee first.",
        },
        "rejected",
        ["privileged-actor-required"],
    ),
    (
        "privileged-negated",
        {"attacker_capability": "A plain EOA; no admin or owner role is needed"},
        "supported",
        [],
    ),
    (
        "privileged-with-escape",
        {"preconditions": "Normally the keeper calls it, but anyone can call sync()."},
        "supported",
        [],
    ),
    (
        "out-of-scope-rules",
        {"program_rules": "In scope:\n- router\n\nOut of scope:\n- vault contracts"},
        "rejected",
        ["out-of-scope-component"],
    ),
    (
        "in-scope-rules",
        {"program_rules": "In scope:\n- vault\n\nOut of scope:\n- governance"},
        "supported",
        [],
    ),
    (
        "out-of-scope-statement",
        {"scope_statement": "The adapter is out of scope but shares the vault math"},
        "rejected",
        ["out-of-scope-component"],
    ),
    (
        "concession-no-loss",
        {
            "reproduction_summary": "1. Call deposit. 2. Call redeem. Note that no funds are lost in this flow."
        },
        "rejected",
        ["concession-contradicts-impact"],
    ),
    (
        "concession-not-profitable",
        {"hypothesis": "The donation attack is not profitable for the attacker at 10,000 USDC."},
        "rejected",
        ["concession-contradicts-impact"],
    ),
    (
        "concession-rebutted",
        {
            "hypothesis": (
                "The team claims no funds are lost, but the PoC shows a victim loses "
                "10,000 USDC."
            )
        },
        "supported",
        [],
    ),
    (
        "self-harm",
        {"mechanism": "Only affects the caller who passes a zero slippage bound by mistake."},
        "rejected",
        ["self-harm-only"],
    ),
    (
        "known-issue-declared",
        {"known_issue_declaration": "Possible duplicate of a listed issue"},
        "held",
        ["known-issue-declared"],
    ),
    (
        "known-issue-in-rules",
        {"program_rules": "Known issues:\n- first depositor share inflation lets attacker steal"},
        "held",
        ["known-issue-declared"],
    ),
    (
        "design-intent",
        {"mechanism": "Rounding toward the vault is by design and documented."},
        "held",
        ["design-intent-likely"],
    ),
    (
        "design-intent-negated",
        {"mechanism": "This rounding is not by design; the spec rounds up."},
        "supported",
        [],
    ),
    (
        "theoretical-hedged",
        {
            "hypothesis": "In theory a victim could lose 10,000 USDC.",
            "reproduction_summary": "",
        },
        "needs_evidence",
        ["theoretical-only"],
    ),
    (
        "no-quantified-impact",
        {
            "hypothesis": "Later depositors lose their deposits.",
            "claimed_impact": "Loss of user funds",
            "reproduction_summary": "Step 1: mint one share. Step 2: donate. Step 3: redeem.",
        },
        "needs_evidence",
        ["no-quantified-impact"],
    ),
    (
        "fabricated-address",
        {"deployed_state_evidence": "Vault live at 0x1234567890abcdef since block 5100"},
        "needs_evidence",
        ["unverified-deployment-evidence"],
    ),
    (
        "no-deployment-evidence",
        {"deployed_state_evidence": ""},
        "needs_evidence",
        ["unverified-deployment-evidence"],
    ),
    (
        "pinned-source-instead",
        {
            "deployed_state_evidence": "",
            "source_ref": {"repo_url": "https://example.test/org/vault", "commit": "a3f9c21e"},
        },
        "supported",
        [],
    ),
    (
        "evidence-without-onchain-ref",
        {"deployed_state_evidence": "The vault is live on mainnet."},
        "needs_evidence",
        ["unverified-deployment-evidence"],
    ),
    (
        "fail-dominates-held",
        {"attacker_capability": "The owner", "known_issue_declaration": "listed"},
        "rejected",
        ["privileged-actor-required", "known-issue-declared"],
    ),
    (
        "held-dominates-needs-evidence",
        {"known_issue_declaration": "listed", "deployed_state_evidence": ""},
        "held",
        ["known-issue-declared", "unverified-deployment-evidence"],
    ),
]


@pytest.mark.parametrize("case_id, overrides, decision, hits", CASES, ids=[c[0] for c in CASES])
def test_golden_stage_a(case_id, overrides, decision, hits):
    result = stage_a(base(**overrides))
    assert result.decision == decision, [
        (r.gate, r.status.value, r.reason) for r in result.gate_results
    ]
    assert result.taxonomy_hits == hits


def test_golden_cases_cover_every_category():
    covered = {hit for _id, _o, _d, hits in CASES for hit in hits}
    assert covered == set(PUBLIC_CATEGORIES)
    assert len(CASES) >= 20


def test_demo_style_fabricated_evidence_never_reaches_supported():
    result = stage_a(base(deployed_state_evidence="Vault live at 0x1234567890abcdef"))
    assert result.decision == "needs_evidence"
    assert "unverified-deployment-evidence" in result.taxonomy_hits
    assert result.penalty_factor < 1


def test_penalty_factor_multiplies_matched_branches():
    clean = stage_a(base())
    assert clean.penalty_factor == 1.0
    two = stage_a(base(attacker_capability="The owner", deployed_state_evidence=""))
    assert two.penalty_factor == pytest.approx(0.2 * 1.0)


def test_benchmark_row_runs_through_stage_a():
    envelope = from_benchmark_row(
        {
            "id": "c1-1",
            "title": "Missing slippage check",
            "hypothesis": "Swaps lack a minimum output, so a sandwich steals 5% of each swap.",
        }
    )
    result = run_stage_a(normalize_envelope_v2(envelope))
    assert result.decision in ("supported", "needs_evidence", "held", "rejected")
    assert result.ruleset_version == "rules-v1.public"


def test_stage_a_reports_the_ruleset_hash():
    ruleset = load_bundled_ruleset("rules-v1.public")
    result = stage_a(base())
    assert result.ruleset_hash == ruleset["payload_sha256"]


def test_stage_a_is_deterministic():
    first = stage_a(base(attacker_capability="The owner"))
    second = stage_a(base(attacker_capability="The owner"))
    assert [(r.gate, r.status, r.reason) for r in first.gate_results] == [
        (r.gate, r.status, r.reason) for r in second.gate_results
    ]
