"""Compatibility checker tests."""

from knowledge_compiler.compat import check


def test_identical_rulesets_are_compatible():
    result = check("rules-v0.synthetic.1", "rules-v0.synthetic.1")
    assert result["compatible"] is True


def test_unknown_ids_fail_closed():
    result = check("rules-v9.private.7", "rules-v0.synthetic.1")
    assert result["compatible"] is False
    assert "unknown ruleset" in result["note"]


def test_v1_public_is_known_but_not_comparable_with_v0():
    assert check("rules-v1.public", "rules-v1.public")["compatible"] is True
    assert check("rules-v0.synthetic.1", "rules-v1.public")["compatible"] is False
    assert check("rules-v1.public", "rules-v0.synthetic.1")["compatible"] is False


def test_note_is_human_readable():
    result = check("rules-v0.synthetic.1", "rules-v0.synthetic.1")
    assert result["note"] == "identical rulesets"
