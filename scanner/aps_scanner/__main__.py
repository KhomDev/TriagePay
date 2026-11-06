"""Command-line scan bridge used by the Rust gateway.

Reads one finding envelope as JSON on stdin and writes the public report
JSON on stdout. Exit codes (unchanged from v1):

- 0: a report was produced (the decision may be any of supported,
     needs_evidence, held, or rejected; payment covers the scan, not the
     verdict);
- 2: the envelope failed validation; stdout carries {"errors": [...]};
- 1: internal error (including a bad command line or a refused report);
     the message goes to stderr.

Engine selection:

- a v1 envelope with no arguments runs the v1 engine and returns the v1
  report, byte for byte as before;
- a v2 envelope (``envelope_version: 2`` or any of hypothesis, source_ref,
  code_context, program_rules) runs the v2 engine: Stage A only;
- ``--sku triage.v2`` runs Stage A plus the Stage B model judge (v1
  envelopes are mapped into v2). Without a configured model, Stage B is
  "unavailable" and the decision is at most needs_evidence;
- ``--engine v1|v2|auto`` forces the engine (auto is the default).
"""

import json
import sys

from .engine import ScanInputError, scan
from .engine_v2 import ScanInputErrorV2, triage
from .envelope_v2 import FindingEnvelopeV2, load_payload
from .public_guard import PublicGuardError

SKU_SCAN_V1 = "scan.v1"
SKU_TRIAGE_V2 = "triage.v2"
SKUS = (SKU_SCAN_V1, SKU_TRIAGE_V2)
ENGINES = ("auto", "v1", "v2")


class UsageError(ValueError):
    pass


def parse_args(argv):
    options = {"sku": SKU_SCAN_V1, "engine": "auto"}
    args = list(argv)
    while args:
        flag = args.pop(0)
        name = flag[2:] if flag.startswith("--") else None
        if name not in options or not args:
            raise UsageError("usage: python -m aps_scanner [--sku SKU] [--engine ENGINE]")
        value = args.pop(0)
        allowed = SKUS if name == "sku" else ENGINES
        if value not in allowed:
            raise UsageError(f"--{name} must be one of {', '.join(allowed)}")
        options[name] = value
    return options


def run(raw, options, judge_loader=None):
    """The CLI pipeline (stdin text in, (exit code, payload) out)."""
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as err:
        return 2, {"errors": ["request body is not valid JSON: " + str(err)]}
    try:
        envelope = load_payload(payload)
    except ValueError as err:
        return 2, {"errors": [str(err)]}
    is_v2 = isinstance(envelope, FindingEnvelopeV2)
    wants_model = options["sku"] == SKU_TRIAGE_V2
    if options["engine"] == "v1" and (is_v2 or wants_model):
        return 2, {"errors": ["v2 fields and the triage.v2 sku require the v2 engine"]}
    use_v2 = is_v2 or wants_model or options["engine"] == "v2"
    if not use_v2:
        try:
            return 0, scan(envelope).to_public_json()
        except ScanInputError as err:
            return 2, {"errors": list(err.errors)}
    judge = None
    if wants_model:
        if judge_loader is None:
            from .stage_b_bridge import load_judge as judge_loader
        judge = judge_loader()
    try:
        return 0, triage(envelope, judge=judge).to_public_json()
    except ScanInputErrorV2 as err:
        return 2, {"errors": list(err.errors)}


def main(argv=None) -> int:
    try:
        options = parse_args(sys.argv[1:] if argv is None else argv)
    except UsageError as err:
        print(f"scanner usage error: {err}", file=sys.stderr)
        return 1
    raw = sys.stdin.read()
    try:
        code, payload = run(raw, options)
    except PublicGuardError as err:
        print(f"scanner refused the report: {err}", file=sys.stderr)
        return 1
    except Exception as err:  # defensive: never crash silently
        print(f"scanner internal error: {err}", file=sys.stderr)
        return 1
    json.dump(payload, sys.stdout, sort_keys=True, ensure_ascii=True)
    return code


if __name__ == "__main__":
    sys.exit(main())
