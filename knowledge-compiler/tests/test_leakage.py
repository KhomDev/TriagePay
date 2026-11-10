"""Leakage tests: poisoned sources abort the compile, no file is written.

Denylisted-term poisons use synthetic stand-in names whose digests sit in a
test-only digest file (tests/fixtures/test-denylist.sha256), loaded through
the same APS_PRIVATE_DENYLIST hook an operator uses for the private list.
Real terms are checked off-repo only: no real private term, and no digest
of one, appears in this repository.
"""

import json
import os
import pathlib
import re
import subprocess

import pytest

from knowledge_compiler import leakage
from knowledge_compiler.compiler import compile_ruleset
from knowledge_compiler.leakage import scan_for_leakage
from knowledge_compiler.schema import SchemaError

SAMPLE = pathlib.Path(__file__).resolve().parent.parent / "samples" / "synthetic"
TEST_DENYLIST = pathlib.Path(__file__).resolve().parent / "fixtures" / "test-denylist.sha256"
REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]

# Synthetic stand-ins: one case-insensitive multi-part term, one
# case-sensitive single-word marker.
STAND_IN_TERM = "zz-private-alpha"
STAND_IN_MARKER = "ZZBRAVO"

# The only names whose digests may be published (knowledge_compiler/denylist.sha256).
PUBLISHED_STAND_INS = ("zz-published-canary",)

# Synthetic guess list for the dictionary-attack test: generic security and
# DeFi vocabulary an attacker would try first. It is NOT a list of private
# terms; the published list must recover none of these.
GUESS_WORDS = (
    "vault", "bridge", "swap", "lend", "borrow", "oracle", "token", "pool",
    "stake", "staking", "dex", "amm", "perp", "lending", "exchange", "wallet",
    "notes", "corpus", "workspace", "doctrine", "private", "secret", "internal",
    "review", "finding", "report", "triage", "scanner", "gateway",
    "ledger", "agent", "hunter", "bounty", "contest", "near", "usdc", "sentinel",
)


def _gate_with_text(text, gate_id="impact_sink"):
    return {
        "id": gate_id,
        "hard": True,
        "public_explanation": text,
        "branches": [{"when": [], "status": "Pass", "reason": "ok"}],
    }


@pytest.fixture
def stand_in_denylist(monkeypatch):
    monkeypatch.setenv(leakage.PRIVATE_DENYLIST_ENV, str(TEST_DENYLIST))
    return leakage.load_denylist()


POISONS = [
    ("denylisted term", "a zz-private-alpha-style rounding bug"),
    ("denylisted marker", "see ZZBRAVO corpus for details"),
    ("denylisted camel case", "the ZzPrivateAlpha pool"),
    ("denylisted path segment", "artifacts under repo/zz_private_alpha/run"),
    ("host path", "repro lives under /Users/whoever/x"),
    ("near account", "funds land in vault.gateway.near"),
    ("secret prefix", "signing key ed25519: then bytes"),
    ("bad url", "snapshot at https://private.example.net/rules"),
]


@pytest.mark.parametrize("name,poison", POISONS, ids=[p[0] for p in POISONS])
def test_poisoned_source_aborts_compile(tmp_path, stand_in_denylist, name, poison):
    gate = _gate_with_text(poison)
    directory = tmp_path / "src"
    directory.mkdir()
    (directory / "source.json").write_text(
        json.dumps({"label": "s", "gates": [gate]}), encoding="utf-8"
    )
    with pytest.raises(SchemaError, match="leakage"):
        compile_ruleset(directory, "r", "0.1.0")
    # Fail closed means nothing was written even on failure paths.
    assert not list((tmp_path / "out").glob("*.json")) if (tmp_path / "out").exists() else True


def test_stand_in_passes_without_the_private_list(monkeypatch):
    # The stand-in is not in the published list: only the private hook
    # makes it a leak, which proves the hook is what loads the digests.
    monkeypatch.delenv(leakage.PRIVATE_DENYLIST_ENV, raising=False)
    assert scan_for_leakage([_gate_with_text("a zz-private-alpha-style bug")], [], []) == []


def test_case_sensitive_marker_ignores_other_casings(stand_in_denylist):
    assert leakage.count_denylisted("see ZZBRAVO here", stand_in_denylist) == 1
    assert leakage.count_denylisted("see zzbravo or Zzbravo here", stand_in_denylist) == 0


def test_case_insensitive_term_matches_every_casing(stand_in_denylist):
    for spelling in ("ZZ-PRIVATE-ALPHA", "Zz Private Alpha", "zz_private_alpha"):
        assert leakage.count_denylisted(spelling, stand_in_denylist) >= 1
    assert leakage.count_denylisted("zz private beta", stand_in_denylist) == 0


def test_scan_for_leakage_clean_for_synthetic_gates():
    artifact = compile_ruleset(SAMPLE, "rules-v0.synthetic.1", "0.1.0")
    findings = scan_for_leakage(artifact["gates"], artifact["migrations"], artifact["provenance"])
    assert findings == []


def test_leakage_finding_reports_path_not_content(stand_in_denylist):
    findings = scan_for_leakage([_gate_with_text("analysis of zz-private-alpha pools")], [], [])
    assert findings == [("denylisted term", "gates")]
    description = leakage.describe_findings(findings)
    assert "zz" not in description.lower()  # neither the term nor the text
    assert "alpha" not in description.lower()


def test_published_denylist_contains_only_digests():
    raw = leakage.DENYLIST_PATH.read_bytes()
    assert raw.endswith(b"\n")
    lines = raw.decode("ascii").splitlines()
    assert lines, "the published denylist is empty"
    assert all(re.fullmatch(r"[0-9a-f]{64}", line) for line in lines)
    assert lines == sorted(set(lines)), "digests must be sorted and unique"
    assert leakage.load_denylist(environ={}) == frozenset(lines)


def test_published_denylist_holds_only_synthetic_stand_ins():
    # Exact allowlist: any digest of a real term added to the published
    # file fails here, because it is not the digest of a known stand-in.
    expected = frozenset(leakage.digest_for_term(name) for name in PUBLISHED_STAND_INS)
    assert leakage.load_denylist(environ={}) == expected


def test_guess_list_recovers_nothing_from_the_published_denylist():
    published = leakage.load_denylist(environ={})
    guesses = {
        variant
        for word in GUESS_WORDS
        for variant in (word, word.upper(), word.capitalize(), word + "s", word.upper() + "S")
    }
    recovered = [guess for guess in guesses if leakage.digest_for_term(guess) in published]
    assert recovered == []


def test_published_canary_aborts_compile_without_the_private_list(tmp_path, monkeypatch):
    # The default (CI) run still exercises the digest path end to end.
    monkeypatch.delenv(leakage.PRIVATE_DENYLIST_ENV, raising=False)
    directory = tmp_path / "src"
    directory.mkdir()
    gate = _gate_with_text("a ZzPublishedCanary rounding bug")
    (directory / "source.json").write_text(
        json.dumps({"label": "s", "gates": [gate]}), encoding="utf-8"
    )
    with pytest.raises(SchemaError, match="denylisted term"):
        compile_ruleset(directory, "r", "0.1.0")


def test_test_denylist_matches_its_stand_ins():
    expected = {leakage.digest_for_term(STAND_IN_TERM), leakage.digest_for_term(STAND_IN_MARKER)}
    assert leakage.read_digest_file(TEST_DENYLIST, "test") == expected


def test_stand_ins_are_not_in_the_published_list():
    published = leakage.load_denylist(environ={})
    assert leakage.digest_for_term(STAND_IN_TERM) not in published
    assert leakage.digest_for_term(STAND_IN_MARKER) not in published


def test_private_denylist_set_but_missing_fails_closed(monkeypatch, tmp_path):
    monkeypatch.setenv(leakage.PRIVATE_DENYLIST_ENV, str(tmp_path / "absent.sha256"))
    with pytest.raises(leakage.DenylistError):
        leakage.load_denylist()


def test_malformed_private_denylist_fails_closed_without_echo(monkeypatch, tmp_path):
    bad = tmp_path / "bad.sha256"
    bad.write_text("not-a-digest-secretword\n", encoding="utf-8")
    monkeypatch.setenv(leakage.PRIVATE_DENYLIST_ENV, str(bad))
    with pytest.raises(leakage.DenylistError) as excinfo:
        leakage.load_denylist()
    assert "line 1" in str(excinfo.value)
    assert "secretword" not in str(excinfo.value)


@pytest.mark.skipif(
    not os.environ.get(leakage.PRIVATE_DENYLIST_ENV),
    reason="local-only: set APS_PRIVATE_DENYLIST to the off-repo digest file",
)
def test_tracked_tree_has_no_private_term():
    # Prints path:line pairs only, never a term.
    listing = subprocess.run(["git", "-C", str(REPO_ROOT), "rev-parse"], capture_output=True)
    if listing.returncode != 0:
        pytest.skip("not a git checkout")
    hits = leakage.scan_tree(REPO_ROOT, leakage.load_private_denylist())
    assert hits == []


def _git_repo_with(tmp_path, files):
    repo = tmp_path / "repo"
    repo.mkdir()
    for relative, text in files.items():
        path = repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(["git", "-C", str(repo), "add", "-A"], check=True)
    return repo


def test_scan_tree_skips_third_party_lockfiles(tmp_path):
    # A dependency name that collides with a generic word on a private list
    # (the synthetic stand-in plays that word here) must not trip the scan
    # inside a lockfile, and must still trip it in ordinary files.
    digests = frozenset({leakage.digest_for_term("zz-private-alpha")})
    dependency_line = 'name = "ed25519-zz-private-alpha-dalek"\n'
    repo = _git_repo_with(
        tmp_path,
        {
            "Cargo.lock": dependency_line,
            "web/package-lock.json": '{"zz-private-alpha": "1.0.0"}\n',
            "py/uv.lock": dependency_line,
            "ui/pnpm-lock.yaml": "zz-private-alpha: 1.0.0\n",
            "src/notes.md": "mentions zz-private-alpha here\n",
        },
    )
    hits = leakage.scan_tree(repo, digests)
    assert hits == [("src/notes.md", 1)]


@pytest.mark.parametrize(
    "relative, expected",
    [
        ("Cargo.lock", True),
        ("crates/x/Cargo.lock", True),
        ("uv.lock", True),
        ("a/package-lock.json", True),
        ("pnpm-lock.yaml", True),
        ("Cargo.toml", False),
        ("package.json", False),
        ("notes/Cargo.lock.md", False),
    ],
)
def test_lockfile_detection_is_by_basename(relative, expected):
    assert leakage.is_lockfile(relative) is expected


def test_scan_public_text_reports_classes_not_text():
    stand_in = frozenset({leakage.digest_for_term("zz-private-alpha")})
    assert leakage.scan_public_text("a generic reason", stand_in) == []
    host_path = leakage.HOST_PATHS[-1] + "someone"
    classes = leakage.scan_public_text("see zz-private-alpha at " + host_path, stand_in)
    assert "denylisted term" in classes
    assert any(c.startswith("host path") for c in classes)
    assert all("zz-private-alpha" not in c for c in classes)
