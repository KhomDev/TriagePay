"""rules-v1.public loading, schema-2 fail-closed checks, and registry sync."""

import copy
import hashlib
import importlib.util
import json
import pathlib

import pytest

from aps_scanner import RulesetError, load_ruleset
from aps_scanner.ruleset import (
    EVALUATORS,
    V2_EVALUATOR_PARAMS,
    default_ruleset,
    default_v2_ruleset,
    load_bundled_ruleset,
)
from aps_scanner.taxonomy import PUBLIC_CATEGORIES

SCHEMA_PATH = (
    pathlib.Path(__file__).resolve().parents[2]
    / "knowledge-compiler"
    / "knowledge_compiler"
    / "schema.py"
)


def _compiler_schema():
    spec = importlib.util.spec_from_file_location("aps_kc_schema_for_tests", SCHEMA_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _rehash(ruleset):
    ruleset["payload_sha256"] = hashlib.sha256(
        json.dumps(ruleset["gates"], sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    return ruleset


def _write(tmp_path, ruleset):
    path = tmp_path / "ruleset.json"
    path.write_text(json.dumps(_rehash(ruleset)), encoding="utf-8")
    return path


def test_both_bundled_rulesets_load():
    assert default_ruleset()["ruleset_id"] == "rules-v0.synthetic.1"
    public = load_bundled_ruleset("rules-v1.public")
    assert public["schema_version"] == 2
    assert public["counts"]["gates"] == 9
    assert {gate["category"] for gate in public["gates"]} == set(PUBLIC_CATEGORIES)


def test_v2_default_is_rules_v1_public():
    assert default_v2_ruleset(environ={})["ruleset_id"] == "rules-v1.public"


def test_v2_override_must_be_schema_2(tmp_path):
    path = tmp_path / "v0.json"
    path.write_text(json.dumps(default_ruleset()), encoding="utf-8")
    with pytest.raises(RulesetError, match="schema 2"):
        default_v2_ruleset(environ={"APS_RULESET_V2_PATH": str(path)})


def test_unknown_bundled_id_fails_closed():
    with pytest.raises(RulesetError):
        load_bundled_ruleset("rules-v9.unknown")


def test_unknown_category_fails_closed(tmp_path):
    ruleset = copy.deepcopy(load_bundled_ruleset("rules-v1.public"))
    ruleset["gates"][0]["category"] = "secret-internal-bucket"
    with pytest.raises(RulesetError, match="taxonomy"):
        load_ruleset(_write(tmp_path, ruleset))


def test_v2_evaluator_in_a_schema_1_ruleset_fails_closed(tmp_path):
    ruleset = copy.deepcopy(default_ruleset())
    ruleset["gates"][0]["branches"][0]["when"] = [
        {"eval": "phrase_affirmed", "params": {"fields": ["title"], "phrases": ["x"]}}
    ]
    with pytest.raises(RulesetError, match="schema 2"):
        load_ruleset(_write(tmp_path, ruleset))


def test_held_status_needs_schema_2(tmp_path):
    ruleset = copy.deepcopy(default_ruleset())
    ruleset["gates"][0]["branches"][0]["status"] = "Held"
    with pytest.raises(RulesetError, match="illegal branch status"):
        load_ruleset(_write(tmp_path, ruleset))


@pytest.mark.parametrize(
    "params",
    [
        None,
        {"fields": ["title"]},
        {"fields": ["title"], "phrases": "not-a-list"},
        {"fields": ["title"], "phrases": ["x"], "bogus": 1},
        {"fields": ["title"], "phrases": ["x"], "window": -1},
    ],
)
def test_malformed_v2_params_fail_closed(tmp_path, params):
    ruleset = copy.deepcopy(load_bundled_ruleset("rules-v1.public"))
    condition = {"eval": "phrase_affirmed"}
    if params is not None:
        condition["params"] = params
    ruleset["gates"][0]["branches"][0]["when"] = [condition]
    with pytest.raises(RulesetError):
        load_ruleset(_write(tmp_path, ruleset))


def test_unknown_evidence_expectation_fails_closed(tmp_path):
    ruleset = copy.deepcopy(load_bundled_ruleset("rules-v1.public"))
    ruleset["gates"][-1]["branches"][0]["when"] = [
        {"eval": "evidence_format", "params": {"fields": ["title"], "expect": "trust-me"}}
    ]
    with pytest.raises(RulesetError, match="expectation"):
        load_ruleset(_write(tmp_path, ruleset))


@pytest.mark.parametrize("penalty", [-0.1, 1.5, True, "0.5"])
def test_illegal_penalty_fails_closed(tmp_path, penalty):
    ruleset = copy.deepcopy(load_bundled_ruleset("rules-v1.public"))
    ruleset["gates"][0]["branches"][0]["penalty"] = penalty
    with pytest.raises(RulesetError, match="penalty"):
        load_ruleset(_write(tmp_path, ruleset))


def test_registries_match_the_compiler():
    schema = _compiler_schema()
    assert schema.EVALUATORS == EVALUATORS
    assert schema.EVALUATOR_PARAMS == V2_EVALUATOR_PARAMS
    assert tuple(schema.PUBLIC_CATEGORIES) == PUBLIC_CATEGORIES


def test_public_ruleset_reasons_name_no_private_markers():
    # Every reason reaches public reports; they must stay generic.
    leakage_path = SCHEMA_PATH.with_name("leakage.py")
    spec = importlib.util.spec_from_file_location("aps_kc_leakage_for_tests", leakage_path)
    leakage = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(leakage)
    ruleset = load_bundled_ruleset("rules-v1.public")
    reasons = " ".join(
        branch["reason"] for gate in ruleset["gates"] for branch in gate["branches"]
    )
    assert leakage.count_denylisted(reasons, leakage.load_denylist()) == 0
