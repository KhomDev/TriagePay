#!/usr/bin/env python3
"""Voice gate: tracked Markdown must not use first-person plural pronouns.

The project has one independent developer, so public text uses first person
singular or neutral voice. This check fails on whole-word first-person
plural pronouns (and their contractions) in every git-tracked *.md file.

Not checked (they are not prose in the project's voice):
- fenced code blocks (``` or ~~~) and inline code spans;
- URLs and Markdown link targets;
- the all-caps token US (a country or region, as in "US-East"), and any
  pronoun-shaped token glued into an identifier by -, _, . or / (as in
  "us-east-1"). Tokens such as USDC never match a whole-word pattern.

Exemption: a line carrying the marker <!-- voice-ok --> is skipped. Use it
only for verbatim quotations of third parties.

Usage (stdlib only, run from anywhere inside the repository):
    python scripts/voice-check.py            # check the tracked tree
    python scripts/voice-check.py FILE...    # check specific files
    python scripts/voice-check.py --self-test
Exit codes: 0 clean, 1 violations found, 2 usage or git error.
"""

import pathlib
import re
import subprocess
import sys

EXEMPT_MARKER = "<!-- voice-ok -->"

# Apostrophe: ASCII or the typographic right single quotation mark (re escape).
TYPOGRAPHIC_APOSTROPHE = chr(0x2019)
APOSTROPHE = "['" + TYPOGRAPHIC_APOSTROPHE + "]"
PRONOUNS = "|".join(
    ["we", "us", "our", "ours", "ourselves"]
    + [f"we{APOSTROPHE}{suffix}" for suffix in ("re", "ve", "ll", "d")]
    + [f"let{APOSTROPHE}s"]
)
PRONOUN_PATTERN = re.compile(
    r"(?<![\w\-./'" + TYPOGRAPHIC_APOSTROPHE + "])("
    + PRONOUNS
    + r")(?![\w'" + TYPOGRAPHIC_APOSTROPHE + r"]|[\-_./]\w)",
    re.IGNORECASE,
)
FENCE_PATTERN = re.compile(r"^\s*(```|~~~)")
INLINE_CODE_PATTERN = re.compile(r"(`+)(?:(?!\1).)+?\1")
URL_PATTERN = re.compile(r"\]\([^)]*\)|<https?://[^>]*>|https?://\S+")


def violations_in_text(text):
    """Return (line_number, token) pairs for each plural first-person token."""
    found = []
    in_fence = False
    fence = ""
    for number, line in enumerate(text.splitlines(), 1):
        match = FENCE_PATTERN.match(line)
        if match:
            if not in_fence:
                in_fence, fence = True, match.group(1)
            elif match.group(1) == fence:
                in_fence = False
            continue
        if in_fence or EXEMPT_MARKER in line:
            continue
        prose = URL_PATTERN.sub(" ", INLINE_CODE_PATTERN.sub(" ", line))
        for token in PRONOUN_PATTERN.findall(prose):
            if token == "US":
                continue
            found.append((number, token))
    return found


def tracked_markdown(root):
    result = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--", "*.md"],
        check=True,
        capture_output=True,
    )
    return sorted(name for name in result.stdout.decode("utf-8").split("\0") if name)


def repo_root():
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True, text=True
    )
    return pathlib.Path(result.stdout.strip())


SELF_TEST_CASES = [
    ("We do not log request bodies.", 1),
    ("This exercises our v2 target.", 1),
    ("Contact us for details.", 1),
    ("Let's ship it; we're close.", 2),
    ("We" + TYPOGRAPHIC_APOSTROPHE + "ve shipped it.", 1),
    ("Settles in USDC on NEAR.", 0),
    ("Hosted in the US region, US-East.", 0),
    ("Deploy to `us-east-1` or us-west-2.", 0),
    ("Run `we --help` first.", 0),
    ("See https://example.org/we/our/us for the spec.", 0),
    ("[link](https://example.org/our-page) text", 0),
    ('"We never do that," the vendor said. <!-- voice-ok -->', 0),
    ("I verified it; the operator approves it.", 0),
    ("The towel was wet; ours is a typo trap: fourth, sweet, trust.", 1),
    ("```\nwe are inside a fence\n```\nafter the fence", 0),
]


def self_test():
    failures = 0
    for text, expected in SELF_TEST_CASES:
        got = len(violations_in_text(text))
        if got != expected:
            failures += 1
            print(f"self-test FAILED: expected {expected}, got {got}: {text!r}")
    print(f"voice-check self-test: {len(SELF_TEST_CASES) - failures}/{len(SELF_TEST_CASES)} ok")
    return 1 if failures else 0


def main(argv):
    if argv[:1] == ["--self-test"]:
        return self_test()
    try:
        root = repo_root()
        files = [pathlib.Path(arg) for arg in argv]
        if not files:
            files = [root / name for name in tracked_markdown(root)]
    except (OSError, subprocess.CalledProcessError) as err:
        print(f"voice-check: cannot list tracked files: {err}", file=sys.stderr)
        return 2
    total = 0
    for path in files:
        text = path.read_text(encoding="utf-8")
        for number, token in violations_in_text(text):
            total += 1
            try:
                shown = path.relative_to(root)
            except ValueError:
                shown = path
            print(f"{shown.as_posix()}:{number}: first-person plural '{token}'")
    print(f"voice-check: {total} violation(s) in {len(files)} Markdown file(s)")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
