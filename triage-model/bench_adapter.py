"""Benchmark adapter: tool_input.jsonl in, tool_output.jsonl out.

Reads the bountiability benchmark's tool input rows ``{id, title,
hypothesis, source_dir}``, maps each into an envelope v2 (title plus
hypothesis; source_dir is ignored), runs the triage engine, and writes one
output row per input row, in input order, in the benchmark's tool output
format:

    {"id": ..., "score": 0..1}                     Stage A only
    {"id": ..., "score": 0..1, "label": "medium"}  with --stage-b (severity call)
    {"id": ..., "failed": true}                    bad row or fail-closed Stage B

A failed row is an unpredicted row for the benchmark, never a 0.0 reject.
The score is the Stage B probability times the Stage A penalty factor, or
the penalty factor alone in Stage A-only mode. Both the factors and the
thresholds are untuned until prompt 05; this adapter makes no accuracy
claim.

Usage:
    python bench_adapter.py --input tool_input.jsonl --output tool_output.jsonl
        [--stage-b] [--limit N]

--stage-b needs a configured model (APS_LIVE_MODEL=1 plus NEARAI_API_KEY,
or APS_TRIAGE_CASSETTES). Output never echoes row text; the console gets
counts only.
"""

import argparse
import json
import os
import pathlib
import sys

import triage_model  # noqa: F401  (side effect: makes aps_scanner importable)
from aps_scanner.engine_v2 import ScanInputErrorV2, triage
from aps_scanner.envelope_v2 import BenchmarkRowError, from_benchmark_row
from triage_model.judge import judge_from_env


def score_row(row, judge=None) -> dict:
    """One output row for one parsed input row (never raises on bad rows)."""
    row_id = row.get("id") if isinstance(row, dict) else None
    try:
        envelope = from_benchmark_row(row)
        report = triage(envelope, judge=judge)
    except (BenchmarkRowError, ScanInputErrorV2):
        return {"id": row_id, "failed": True}
    if report.score is None:
        return {"id": row_id, "failed": True}
    out = {"id": row_id, "score": report.score}
    severity = report.stage_b.get("severity_call") if report.stage_b["status"] == "ok" else None
    if severity:
        out["label"] = severity
    return out


def adapt(input_path, output_path, judge=None, limit=None) -> dict:
    input_path = pathlib.Path(input_path)
    output_path = pathlib.Path(output_path)
    if input_path.resolve() == output_path.resolve():
        raise ValueError("the output must not overwrite the input")
    counts = {"rows": 0, "scored": 0, "failed": 0, "labelled": 0}
    rows_out = []
    with input_path.open(encoding="utf-8") as handle:
        for line in handle:
            if not line.strip():
                continue
            if limit is not None and counts["rows"] >= limit:
                break
            counts["rows"] += 1
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                row = None
            out = score_row(row, judge) if row is not None else {"id": None, "failed": True}
            counts["failed" if out.get("failed") else "scored"] += 1
            counts["labelled"] += 1 if "label" in out else 0
            rows_out.append(out)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    temporary = output_path.with_name(output_path.name + ".partial")
    with temporary.open("w", encoding="utf-8", newline="\n") as handle:
        for out in rows_out:
            handle.write(json.dumps(out, sort_keys=True) + "\n")
    os.replace(temporary, output_path)
    return counts


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="bench_adapter")
    parser.add_argument("--input", required=True, help="the benchmark's tool_input.jsonl")
    parser.add_argument("--output", required=True, help="where to write tool_output.jsonl")
    parser.add_argument("--stage-b", action="store_true", help="also run the model judge")
    parser.add_argument("--limit", type=int, default=None, help="score at most N rows")
    args = parser.parse_args(argv)
    judge = None
    if args.stage_b:
        judge = judge_from_env()
        if judge is None:
            print("bench_adapter: --stage-b needs a configured model", file=sys.stderr)
            return 2
    try:
        counts = adapt(args.input, args.output, judge=judge, limit=args.limit)
    except (OSError, ValueError) as err:
        print(f"bench_adapter: {err}", file=sys.stderr)
        return 2
    print("bench_adapter: " + ", ".join(f"{k} {v}" for k, v in counts.items()))
    return 0


if __name__ == "__main__":
    sys.exit(main())
