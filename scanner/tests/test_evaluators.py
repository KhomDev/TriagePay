"""Stage A evaluator primitives: positive, negative and negation cases."""

import pytest

from aps_scanner import textmatch as tm

# --- negation-aware phrase matching -----------------------------------------


@pytest.mark.parametrize(
    "text, expected",
    [
        ("Funds are lost when the oracle is stale.", True),
        ("No funds are lost in this flow.", False),
        ("The attacker never steals anything; funds are lost later.", True),
        ("This does not cause funds are lost style damage.", False),
        ("Funds aren't lost.", False),  # an in-phrase negation is simply no match
        ("Funds are lost; nobody notices.", True),  # a negator AFTER the phrase does not negate it
    ],
)
def test_phrase_negation(text, expected):
    assert tm.phrase_affirmed(text, "funds are lost") is expected


def test_negation_window_is_bounded():
    far = "No one expected that after many many many blocks funds are lost."
    assert tm.phrase_affirmed(far, "funds are lost") is True
    assert tm.phrase_affirmed(far, "funds are lost", window=20) is False


def test_contrastive_conjunction_ends_the_negation_scope():
    text = "No admin is needed, but the owner must sign the upgrade."
    assert tm.phrase_affirmed(text, "admin") is False
    assert tm.phrase_affirmed(text, "owner must") is True


def test_contractions_negate():
    assert tm.phrase_affirmed("It doesn't require the owner.", "the owner") is False
    assert tm.phrase_affirmed("It can’t reach the owner path.", "the owner") is False


def test_phrase_matching_is_token_based_not_substring():
    assert tm.phrase_affirmed("the downer path", "owner") is False
    assert tm.any_phrase_affirmed("Out-of-scope component", ["out of scope"]) is True


# --- precondition-privilege classification ----------------------------------

CAP_TERMS = ["admin", "owner", "governance", "trusted keeper"]
CTX = ["requires the owner", "malicious admin", "admin must"]
ESC = ["permissionless", "any user", "anyone", "no special role"]


def privileged(capability="", context="", escape=None):
    return tm.privilege_required(
        capability, CAP_TERMS, context, CTX, capability + "\n" + context if escape is None else escape, ESC
    )


def test_privilege_positive_from_capability_field():
    assert privileged(capability="The contract owner") is True


def test_privilege_positive_from_context_phrase():
    assert privileged(context="The exploit requires the owner to call pause().") is True


def test_privilege_negative_for_an_ordinary_attacker():
    assert privileged(capability="Any external caller with collateral") is False


def test_privilege_negated_role_is_not_privileged():
    assert privileged(capability="No admin or owner involvement; a plain EOA") is False


def test_privilege_permissionless_escape_wins():
    assert privileged(capability="The owner, or anyone, can trigger the sync") is False


def test_privilege_negated_escape_does_not_count():
    assert privileged(capability="The owner; the path is not permissionless") is True


def test_privilege_bare_role_word_in_context_is_not_enough():
    # "admin fee" in the hypothesis is not a requirement on the attacker.
    assert privileged(context="The bug misroutes the admin fee to the pool.") is False


# --- quantification ----------------------------------------------------------

IMPACT = ["lose", "lost", "loss", "drain", "drains", "drained", "steal", "stolen", "mint"]
UNITS = ["eth", "usdc", "tokens", "wei"]
ABSOLUTE = ["all deposits", "entire balance"]


def quantified(text, window=12):
    return tm.quantified_impact(text, IMPACT, UNITS, ABSOLUTE, window)


@pytest.mark.parametrize(
    "text",
    [
        "The attacker drains 1,200 ETH from the pool.",
        "Users lose 35% of their deposits.",
        "A loss of $250k accrues per epoch.",
        "Victims lose 10000 USDC each.",
        "The attacker can mint 1e24 tokens.",
        "The bug drains all deposits.",
    ],
)
def test_quantification_positive(text):
    assert quantified(text) is True


@pytest.mark.parametrize(
    "text",
    [
        "Users lose funds.",
        "The pool holds 1,200 ETH. Nothing else happens.",  # quantity not bound to a loss
        "Interest accrues at 5% per year.",
    ],
)
def test_quantification_negative(text):
    assert quantified(text) is False


def test_quantification_negated_impact_does_not_count():
    assert quantified("No funds are lost even with 500 ETH deposited.") is False


def test_quantification_respects_the_window():
    text = "Users lose " + "word " * 30 + "eventually 5 ETH is mentioned."
    assert quantified(text) is False
    assert quantified(text, window=40) is True


# --- concession detection ----------------------------------------------------

CONCESSIONS = ["no funds are lost", "not profitable", "only fee drag", "user error"]


@pytest.mark.parametrize(
    "text",
    [
        "Note that no funds are lost in this flow.",
        "The attack is not profitable for the attacker.",
        "The effect is only fee drag on the vault.",
        "This needs user error to trigger.",
    ],
)
def test_concession_positive(text):
    assert tm.concession_present(text, CONCESSIONS) is True


@pytest.mark.parametrize(
    "text",
    [
        "The sponsor claims no funds are lost, but the PoC drains 10 ETH.",
        "Even if no funds are lost directly, accounting breaks.",
        "Funds are lost on every call.",
    ],
)
def test_concession_negative_or_rebutted(text):
    assert tm.concession_present(text, CONCESSIONS) is False


def test_concession_negated_admission_is_not_a_concession():
    assert tm.concession_present("It is not user error: any caller triggers it.", CONCESSIONS) is False


# --- scope cross-check -------------------------------------------------------

MARKERS = ["out of scope", "excluded"]


def test_scope_match_multiline_section():
    rules = "In scope:\n- vault\n\nOut of scope:\n- governance module\n- oracle adapters"
    assert tm.rules_section_match(rules, MARKERS, "oracle", 1) is True
    assert tm.rules_section_match(rules, MARKERS, "vault", 1) is False


def test_scope_match_single_line_sentences():
    rules = "Out of scope: issues in the governance module. In scope: the vault."
    assert tm.rules_section_match(rules, MARKERS, "governance", 1) is True
    assert tm.rules_section_match(rules, MARKERS, "vault", 1) is False


def test_scope_negated_marker_opens_no_section():
    rules = "Bridges are not out of scope."
    assert tm.rules_section_match(rules, MARKERS, "bridges", 1) is False


def test_scope_without_rules_or_component_never_matches():
    assert tm.rules_section_match("", MARKERS, "vault", 1) is False
    assert tm.rules_section_match("Out of scope: vault", MARKERS, "", 1) is False


def test_scope_min_overlap():
    rules = "Known issues:\n- stale price feed allows over-borrowing"
    title = "Stale price feed lets borrowers over-borrow"
    assert tm.rules_section_match(rules, ["known issues"], title, 3) is True
    assert tm.rules_section_match(rules, ["known issues"], "Reentrancy in withdraw", 3) is False


# --- evidence-reference format checks ---------------------------------------

REAL_ADDRESS = "0x5f3b8a1c9d2e47f6a0b1c2d3e4f5a6b7c8d9e0f1"
REAL_TX = "0x" + "9c4e1a7b3f5d2e8c" * 4


def test_fabricated_short_hex_is_a_placeholder():
    assert tm.evidence_placeholder("Vault live at 0x1234567890abcdef since block 5100") is True
    assert tm.evidence_onchain_ref("Vault live at 0x1234567890abcdef since block 5100") is False


@pytest.mark.parametrize(
    "text",
    [
        "Deployed at 0x" + "1234567890abcdef" * 2 + "12345678",  # counting run, 40 hex
        "Deployed at 0x" + "deadbeef" * 5,
        "Deployed at 0x" + "0" * 40,
        "Deployed at 0x... (address TBD)",
        "Deployed at <address>",
    ],
)
def test_low_entropy_and_marker_placeholders(text):
    assert tm.evidence_placeholder(text) is True


def test_well_formed_references_pass_the_format_check():
    assert tm.evidence_placeholder(f"Vault at {REAL_ADDRESS}") is False
    assert tm.evidence_onchain_ref(f"Vault at {REAL_ADDRESS}") is True
    assert tm.evidence_onchain_ref(f"tx {REAL_TX}") is True
    assert tm.evidence_onchain_ref("https://explorer.example.org/tx/9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin") is True


def test_text_without_references_is_neither():
    assert tm.evidence_placeholder("live since last week") is False
    assert tm.evidence_onchain_ref("live since last week") is False


@pytest.mark.parametrize(
    "source_ref, expected",
    [
        ({"repo_url": "https://example.test/org/repo", "commit": "a3f9c21"}, True),
        ({"repo_url": "https://example.test/org/repo", "commit": "9b1e4d07c2a6f3e85d1b"}, True),
        ({"repo_url": "http://example.test/org/repo", "commit": "a3f9c21"}, False),
        ({"repo_url": "https://example.test/org/repo", "commit": "1234567"}, False),
        ({"repo_url": "https://example.test/org/repo", "commit": "0000000"}, False),
        ({"repo_url": "https://example.test/org/repo", "commit": "main"}, False),
        (None, False),
    ],
)
def test_pinned_source(source_ref, expected):
    assert tm.pinned_source(source_ref) is expected
