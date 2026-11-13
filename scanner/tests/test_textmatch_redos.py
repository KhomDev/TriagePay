"""Red-team RT-09: the quantity matchers run in linear time on digit runs."""

import time

from aps_scanner.textmatch import quantified_impact

IMPACT = ["lose", "drain", "steal"]
UNITS = ["usdc", "eth", "tokens"]
ABSOLUTE = ["all deposits"]


def _elapsed(text):
    start = time.perf_counter()
    quantified_impact(text, IMPACT, UNITS, ABSOLUTE)
    return time.perf_counter() - start


def test_long_digit_runs_do_not_backtrack():
    # Before the fix "1" * 12000 took about five seconds.
    for text in ("lose " + "1" * 12000, "lose " + "1," * 6000, "lose " + "1.5" * 4000):
        assert _elapsed(text) < 0.5, text[:20]


def test_quantities_still_match():
    assert quantified_impact("An attacker can drain 50% of the pool.", IMPACT, UNITS, ABSOLUTE)
    assert quantified_impact("They lose 1,000,000 usdc in one call.", IMPACT, UNITS, ABSOLUTE)
    assert quantified_impact("Users lose $12.5m at once.", IMPACT, UNITS, ABSOLUTE)
    assert not quantified_impact("Nothing to see in 50 words.", IMPACT, UNITS, ABSOLUTE)
