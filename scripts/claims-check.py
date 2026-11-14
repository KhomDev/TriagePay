#!/usr/bin/env python3
"""Fail when docs/claims-audit.md cites a test or function that does not exist.

Every backticked snake_case name in a table row's Evidence column (two or
more underscores, no dots or slashes) must resolve to a definition in the
repository: `fn name` (Rust), `def name` (Python), or a JS/TS function or
test of that name. A ledger whose GREEN rows cite missing tests is worse than
no ledger (Trail of Bits skill run, 2026-11-14, C-SC-5).

Usage: python scripts/claims-check.py [repo root]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

NAME = re.compile(r"`([a-z][a-z0-9]*(?:_[a-z0-9]+){2,})`")
SKIP_DIRS = {".git", "target", "node_modules", ".venv", "__pycache__", "dist"}
SOURCE_SUFFIXES = {".rs", ".py", ".js", ".mjs", ".ts"}


def cited_names(ledger: str) -> dict[str, list[int]]:
    """Map each cited name to the ledger lines that cite it."""
    cited: dict[str, list[int]] = {}
    for number, line in enumerate(ledger.splitlines(), start=1):
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if not line.lstrip().startswith("|") or len(cells) < 3:
            continue
        if set(cells[0]) <= {"-", " ", ":"} or cells[0] in {"Claim", "Suite"}:
            continue
        for name in NAME.findall(cells[1]):
            cited.setdefault(name, []).append(number)
    return cited


def defined_names(root: Path) -> set[str]:
    """Names a citation may resolve to: function and test definitions,
    macro-generated tests, test module names, and quoted step ids that a
    recorded transcript uses (`record({ step: "over_per_call" })`)."""
    definition = re.compile(r"\b(?:fn|def|function)\s+([a-z][a-z0-9_]*)")
    macro_test = re.compile(r"!\(\s*([a-z][a-z0-9_]+)\s*,")
    quoted = re.compile(r"[\"']([a-z][a-z0-9]*(?:_[a-z0-9]+){2,})[\"']")
    found: set[str] = set()
    for path in root.rglob("*"):
        if not path.is_file() or any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue
        if path.suffix not in SOURCE_SUFFIXES:
            continue
        found.add(path.stem)
        text = path.read_text(encoding="utf-8", errors="replace")
        for pattern in (definition, macro_test, quoted):
            found.update(pattern.findall(text))
    return found


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    ledger = (root / "docs" / "claims-audit.md").read_text(encoding="utf-8")
    cited = cited_names(ledger)
    defined = defined_names(root)
    missing = {name: lines for name, lines in cited.items() if name not in defined}
    for name, lines in sorted(missing.items()):
        where = ", ".join(str(n) for n in lines)
        print(f"docs/claims-audit.md:{where}: `{name}` is cited but not defined")
    print(f"claims-check: {len(cited)} cited names, {len(missing)} unresolved")
    return 1 if missing else 0


if __name__ == "__main__":
    raise SystemExit(main())
