"""Command-line scan bridge used by the Rust gateway.

Reads one finding envelope as JSON on stdin and writes the public report
JSON on stdout. Exit codes:

- 0: a report was produced (the decision may be any of supported,
     needs_evidence, held, or rejected; payment covers the scan, not the
     verdict);
- 2: the envelope failed validation; stdout carries {"errors": [...]};
- 1: internal error; the message goes to stderr.
"""

import json
import sys

from .engine import ScanInputError, scan
from .schema import FindingEnvelope

_ENVELOPE_FIELDS = {
    "title",
    "claimed_impact",
    "mechanism",
    "attacker_capability",
    "preconditions",
    "component_class",
    "scope_statement",
    "deployed_state_evidence",
    "public_rules_snapshot",
    "known_issue_declaration",
    "reproduction_summary",
    "evidence_refs",
    "code_excerpt",
}


def _load_envelope(raw: str) -> FindingEnvelope:
    """Parse and shallow-check the request body.

    Field-level validation (types, lengths, counts) is the schema's job;
    this loader only rejects bodies that could not become an envelope at
    all: non-JSON, non-object, unknown fields, bad evidence_refs shape.
    """
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as err:
        raise ValueError("request body is not valid JSON: " + str(err)) from err
    if not isinstance(payload, dict):
        raise ValueError("request body must be a JSON object")
    unknown = sorted(set(payload) - _ENVELOPE_FIELDS)
    if unknown:
        raise ValueError("unknown envelope fields: " + ", ".join(unknown))
    refs = payload.get("evidence_refs", [])
    if not isinstance(refs, list) or not all(isinstance(r, str) for r in refs):
        raise ValueError("evidence_refs must be a list of strings")
    return FindingEnvelope(
        title=payload.get("title", ""),
        claimed_impact=payload.get("claimed_impact", ""),
        mechanism=payload.get("mechanism", ""),
        attacker_capability=payload.get("attacker_capability", ""),
        preconditions=payload.get("preconditions", ""),
        component_class=payload.get("component_class", ""),
        scope_statement=payload.get("scope_statement", ""),
        deployed_state_evidence=payload.get("deployed_state_evidence", ""),
        public_rules_snapshot=payload.get("public_rules_snapshot", ""),
        known_issue_declaration=payload.get("known_issue_declaration"),
        reproduction_summary=payload.get("reproduction_summary", ""),
        evidence_refs=list(refs),
        code_excerpt=payload.get("code_excerpt"),
    )


def main() -> int:
    raw = sys.stdin.read()
    try:
        envelope = _load_envelope(raw)
    except ValueError as err:
        json.dump({"errors": [str(err)]}, sys.stdout, sort_keys=True)
        return 2
    try:
        report = scan(envelope)
    except ScanInputError as err:
        json.dump({"errors": list(err.errors)}, sys.stdout, sort_keys=True)
        return 2
    except Exception as err:  # defensive: never crash silently
        print(f"scanner internal error: {err}", file=sys.stderr)
        return 1
    json.dump(
        report.to_public_json(),
        sys.stdout,
        sort_keys=True,
        ensure_ascii=True,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

