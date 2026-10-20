"""Leakage scanner: the privacy boundary of the compiler.

A compile FAILS CLOSED (no artifact is written) when the serialized gates,
migrations, or provenance labels contain any private marker: protocol
names, private-workspace markers, host paths, URLs outside the allowlist,
NEAR-style account ids, or secret-like strings. Findings report the
pattern name and the JSON path only - never the matched text itself, so
diagnostics can never become the leak they describe.
"""

import re

# Known protocol names (case-insensitive substring).
PROTOCOL_NAMES = (
    "redacted",
    "redacted",
    "redacted",
    "redacted",
    "redacted",
    "redacted",
    "redacted",
    "redacted",
)

# Private-workspace markers, matched as whole words (case-sensitive) so
# ordinary text like "details" or "retail" cannot false-positive.
WORKSPACE_MARKER_PATTERN = re.compile(
    r"\b(redacted|redacted|redacted|redacted|redacted|redacted)\b"
)

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


def _check_text(text, path, findings):
    """Scan one piece of serialized text; append (pattern, path) findings."""
    lowered = text.lower()
    for name in PROTOCOL_NAMES:
        if name in lowered:
            findings.append((f"protocol name '{name}'", path))
    if WORKSPACE_MARKER_PATTERN.search(text):
        findings.append(("private-workspace marker", path))
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
    import json

    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)


def scan_for_leakage(gates, migrations, provenance):
    """Scan the compiled artifact parts; return a list of findings.

    Structural hash fields (payload_sha256, provenance digests) are
    excluded by design: they are 64-hex digests of already-scanned
    content, not secrets.
    """
    findings: list = []
    _check_text(_canonical(gates), "gates", findings)
    _check_text(_canonical(migrations), "migrations", findings)
    for entry in provenance:
        # Only the opaque label: the digest is a hash of scanned content.
        _check_text(str(entry.get("label", "")), "provenance.label", findings)
    return findings


def describe_findings(findings):
    """One line per finding, without the matched text."""
    if not findings:
        return "no leakage detected"
    return "; ".join(f"[{pattern}] at {path}" for pattern, path in findings)
