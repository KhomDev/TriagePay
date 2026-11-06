"""The public triage taxonomy: the only category ids a report may carry.

This list is the public contract (docs/engine/taxonomy.md). Every gate of
a v2 ruleset, public or private, maps onto exactly one of these ids, and
the Stage B model may only name ids from this list. Order is the display
order in reports.
"""

PUBLIC_CATEGORIES = (
    "privileged-actor-required",
    "no-quantified-impact",
    "self-harm-only",
    "theoretical-only",
    "design-intent-likely",
    "out-of-scope-component",
    "known-issue-declared",
    "concession-contradicts-impact",
    "unverified-deployment-evidence",
)

CATEGORY_SUMMARIES = {
    "privileged-actor-required": (
        "the attack needs a privileged actor and names no permissionless route"
    ),
    "no-quantified-impact": (
        "the impact is not stated as an amount, percentage or token quantity"
    ),
    "self-harm-only": "only the caller's own position is harmed, or the loss needs user error",
    "theoretical-only": "the impact is hedged and no concrete reproduction is supplied",
    "design-intent-likely": "the behaviour reads as documented or intended design",
    "out-of-scope-component": (
        "the component matches an exclusion in the supplied program rules"
    ),
    "known-issue-declared": "the finding is declared or listed as a known issue",
    "concession-contradicts-impact": (
        "the finding concedes no loss or no profit while claiming value impact"
    ),
    "unverified-deployment-evidence": (
        "the deployment reference is absent, malformed or not verifiable"
    ),
}

_ORDER = {category: index for index, category in enumerate(PUBLIC_CATEGORIES)}


def is_public_category(value) -> bool:
    return isinstance(value, str) and value in _ORDER


def ordered(categories) -> list:
    """Distinct public categories in display order (unknown ids dropped)."""
    return sorted({c for c in categories if is_public_category(c)}, key=_ORDER.__getitem__)
