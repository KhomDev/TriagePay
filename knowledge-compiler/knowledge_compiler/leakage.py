"""Leakage scanner: the privacy boundary of the compiler.

A compile FAILS CLOSED (no artifact is written) when the serialized gates,
migrations, or provenance labels contain any private marker: a denylisted
term, a host path, a URL outside the allowlist, a NEAR-style account id,
or a secret-like string. Findings report the pattern class and the JSON
path only - never the matched text itself, so diagnostics can never become
the leak they describe.

Real denylisted terms (private protocol, project, and workspace names) are
checked off-repo only. They are never written down in this repository, not
even as digests: a published SHA-256 digest of a short, guessable word is
reversible by hashing candidate words, so publishing it would publish the
word. The operator keeps the real digests in a private file (see
``APS_PRIVATE_DENYLIST`` below); CI and every default run check only the
structural patterns plus the synthetic stand-ins.

The published ``denylist.sha256`` next to this module holds only digests of
synthetic stand-in names (``zz-published-canary``) so the mechanism is
exercised in every run. The format is one lowercase 64-hex digest per line.
The checker tokenizes candidate text into word, path-segment, camel-case,
and joined multi-part variants and compares the digest of each variant
against the loaded digests.

Normalization rule for a term before hashing (see ``normalize_term``):
NFKC, separators (whitespace, ``_``, ``.``, ``/``, ``\\``) folded to ``-``.
A term written entirely in lowercase is case-insensitive (it matches any
casing of the token); a term containing an uppercase letter is
case-sensitive (it matches that exact casing only, so an all-caps marker
never trips on the ordinary lowercase English word). Joined variants are
compared in lowercase only, so a multi-part term must be written in
lowercase; a case-sensitive term must be a single word.

Private list: the environment variable ``APS_PRIVATE_DENYLIST`` names a
file outside the repository with the real digests in the same format. The
compiler, ``scan-tree`` (which checks only this list), and the privacy
tests load it when the variable is
set; CI runs without it, so real-term checks run only on the operator's
host. A set-but-missing or malformed file fails closed. Even off-repo, a
digest only spares the file from holding the plaintext; anyone who obtains
the private file can recover guessable terms by dictionary hashing.

Generic structural patterns (host-path prefixes, account-id shapes,
secret-key shapes) stay literal below: they reveal nothing.

Operator helpers (terms are read from stdin, never from argv, so they do
not land in shell history; output never echoes a term):

    python -m knowledge_compiler digest < terms.txt
    python -m knowledge_compiler scan-tree --root <repo-root> [--require-private]
"""

import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import unicodedata

DENYLIST_PATH = pathlib.Path(__file__).with_name("denylist.sha256")
PRIVATE_DENYLIST_ENV = "APS_PRIVATE_DENYLIST"
DIGEST_LINE_PATTERN = re.compile(r"[0-9a-f]{64}")

# Longest joined variant checked, in word parts ("a-b-c-d" is 4).
MAX_JOINED_PARTS = 4

_WORD_PATTERN = re.compile(r"[A-Za-z0-9]+")
_CAMEL_PART_PATTERN = re.compile(r"[A-Z]+(?![a-z])|[A-Z]?[a-z]+|[0-9]+")
_SEPARATOR_PATTERN = re.compile(r"[\s_./\\-]+")

HOST_PATHS = ("C:\\", "/Users/", "/home/")

# URLs: allowed hosts only. The legitimate synthetic ruleset contains the
# literal prefixes "http://" and "https://" (a starts_with check), which
# do not match the URL pattern because no host follows.
URL_PATTERN = re.compile(r"https?://([^/\s\"]+)")
ALLOWED_HOSTS = frozenset({"archive.example.org", "example.test", "example.org"})

# NEAR-style account ids (name.name.near or 64-hex implicit accounts).
NEAR_ACCOUNT_PATTERN = re.compile(r"\b[a-z0-9_-]+(?:\.[a-z0-9_-]+)*\.near\b")
IMPLICIT_ACCOUNT_PATTERN = re.compile(r"\b[0-9a-f]{64}\b")

# Secret-like strings.
KEY_PREFIXES = ("ed25519:", "secp256k1:")
BASE58_RUN_PATTERN = re.compile(r"\b[1-9A-HJ-NP-Za-km-z]{40,}\b")
HEX_RUN_PATTERN = re.compile(r"\b0x[0-9a-fA-F]{40,}\b")


class DenylistError(ValueError):
    """A denylist file is missing or malformed (the compile fails closed)."""


def digest(token):
    """SHA-256 hex digest of one already-normalized token."""
    return hashlib.sha256(token.encode("utf-8")).hexdigest()


def normalize_term(term):
    """Normalize a denylist term exactly as the checker normalizes tokens."""
    text = unicodedata.normalize("NFKC", term).strip()
    return _SEPARATOR_PATTERN.sub("-", text).strip("-")


def digest_for_term(term):
    """The denylist line for a term (see the module docstring for casing)."""
    return digest(normalize_term(term))


def parse_digest_lines(text, source="denylist"):
    """Parse a digest file body: one lowercase 64-hex digest per line.

    Blank lines are ignored; anything else fails closed. The error names
    the line number only, never the line content.
    """
    digests = set()
    for number, line in enumerate(text.splitlines(), 1):
        line = line.strip()
        if not line:
            continue
        if not DIGEST_LINE_PATTERN.fullmatch(line):
            raise DenylistError(f"{source}: line {number} is not a 64-hex digest")
        digests.add(line)
    return frozenset(digests)


def read_digest_file(path, source):
    try:
        text = pathlib.Path(path).read_text(encoding="utf-8")
    except OSError as err:
        raise DenylistError(f"{source}: cannot read the digest file ({err.strerror})") from err
    return parse_digest_lines(text, source)


def load_private_denylist(environ=None):
    """The real-term digests from APS_PRIVATE_DENYLIST, or empty when unset."""
    environ = os.environ if environ is None else environ
    private_path = environ.get(PRIVATE_DENYLIST_ENV, "").strip()
    if not private_path:
        return frozenset()
    return read_digest_file(private_path, PRIVATE_DENYLIST_ENV)


def load_denylist(environ=None, published_path=None):
    """Published stand-in digests, plus the private file named by APS_PRIVATE_DENYLIST."""
    digests = set(read_digest_file(published_path or DENYLIST_PATH, "published denylist"))
    return frozenset(digests | load_private_denylist(environ))


def token_variants(text):
    """Every candidate token in the text, in exact and lowercase forms.

    Variants: whole words, camel-case and letter/digit parts, and joined
    variants of 2..MAX_JOINED_PARTS consecutive parts ("a-b", "a-b-c"),
    which covers hyphenated, underscored, spaced, path-segment, and
    camel-case spellings of a multi-part term.
    """
    text = unicodedata.normalize("NFKC", text)
    variants = set()
    parts = []
    for word in _WORD_PATTERN.findall(text):
        variants.add(word)
        word_parts = _CAMEL_PART_PATTERN.findall(word) or [word]
        variants.update(word_parts)
        parts.extend(part.lower() for part in word_parts)
    for size in range(2, MAX_JOINED_PARTS + 1):
        for start in range(len(parts) - size + 1):
            variants.add("-".join(parts[start : start + size]))
    variants.update([variant.lower() for variant in variants])
    return variants


def count_denylisted(text, digests):
    """How many distinct token variants of the text are denylisted.

    Returns a count only, so a caller cannot accidentally print the term.
    """
    if not digests:
        return 0
    return sum(1 for variant in token_variants(text) if digest(variant) in digests)


def _check_text(text, path, findings, digests):
    """Scan one piece of serialized text; append (pattern, path) findings."""
    if count_denylisted(text, digests):
        findings.append(("denylisted term", path))
    for host_path in HOST_PATHS:
        if host_path in text:
            findings.append((f"host path '{host_path}'", path))
    for match in URL_PATTERN.finditer(text):
        host = match.group(1)
        if host not in ALLOWED_HOSTS:
            findings.append((f"non-allowlisted URL host '{host}'", path))
    if NEAR_ACCOUNT_PATTERN.search(text):
        findings.append(("NEAR account id", path))
    if IMPLICIT_ACCOUNT_PATTERN.search(text):
        findings.append(("64-hex implicit account", path))
    for prefix in KEY_PREFIXES:
        if prefix in text:
            findings.append((f"key prefix '{prefix}'", path))
    if BASE58_RUN_PATTERN.search(text):
        findings.append(("long base58 run", path))
    if HEX_RUN_PATTERN.search(text):
        findings.append(("long 0x-hex run", path))


def _canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def scan_for_leakage(gates, migrations, provenance, denylist=None):
    """Scan the compiled artifact parts; return a list of findings.

    ``denylist`` defaults to ``load_denylist()`` (published stand-ins plus the
    optional private file). Structural hash fields (payload_sha256,
    provenance digests) are excluded by design: they are 64-hex digests of
    already-scanned content, not secrets.
    """
    digests = load_denylist() if denylist is None else denylist
    findings: list = []
    _check_text(_canonical(gates), "gates", findings, digests)
    _check_text(_canonical(migrations), "migrations", findings, digests)
    for entry in provenance:
        # Only the opaque label: the digest is a hash of scanned content.
        _check_text(str(entry.get("label", "")), "provenance.label", findings, digests)
    return findings


def describe_findings(findings):
    """One line per finding, without the matched text."""
    if not findings:
        return "no leakage detected"
    return "; ".join(f"[{pattern}] at {path}" for pattern, path in findings)


def scan_tree(root, digests):
    """Scan every git-tracked text file under root; return (path, line) hits."""
    listing = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"],
        check=True,
        capture_output=True,
    ).stdout.decode("utf-8")
    hits = []
    for relative in sorted(name for name in listing.split("\0") if name):
        try:
            text = (pathlib.Path(root) / relative).read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue  # binary or unreadable: out of scope for a text scan
        for number, line in enumerate(text.splitlines(), 1):
            if count_denylisted(line, digests):
                hits.append((relative, number))
    return hits


def main(argv=None):
    import argparse

    parser = argparse.ArgumentParser(prog="knowledge_compiler")
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("digest", help="print the digest of each stdin line (a term)")
    tree = subparsers.add_parser("scan-tree", help="scan git-tracked files for denylisted terms")
    tree.add_argument("--root", default=".", help="repository root")
    tree.add_argument(
        "--require-private",
        action="store_true",
        help=f"fail unless {PRIVATE_DENYLIST_ENV} is set",
    )
    args = parser.parse_args(argv)

    if args.command == "digest":
        for line in sys.stdin.read().splitlines():
            if line.strip():
                print(digest_for_term(line))
        return 0

    if args.require_private and not os.environ.get(PRIVATE_DENYLIST_ENV, "").strip():
        print(f"scan-tree: {PRIVATE_DENYLIST_ENV} is not set", file=sys.stderr)
        return 2
    # Real terms only: the published stand-ins legitimately appear in the
    # tests and docs that exercise them, so they are not a tree leak.
    try:
        digests = load_private_denylist()
    except DenylistError as err:
        print(f"scan-tree: {err}", file=sys.stderr)
        return 2
    if not digests:
        print(f"scan-tree: {PRIVATE_DENYLIST_ENV} is not set; no real terms to check")
        return 0
    hits = scan_tree(args.root, digests)
    for relative, number in hits:
        print(f"{relative}:{number}: denylisted term")
    files = len({relative for relative, _ in hits})
    print(f"scan-tree: {len(hits)} hit(s) in {files} file(s) against {len(digests)} digest(s)")
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main())
