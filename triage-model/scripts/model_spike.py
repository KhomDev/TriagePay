"""Model-selection spike: schema compliance, latency, tokens and cost per call.

Runs the Stage B judge live over benchmark-shaped rows ``{id, title,
hypothesis}`` for each candidate model and writes an aggregate JSON
summary. Measurements come from the API's own usage fields; cost is
usage times the catalogue price passed on the command line. The run stops
before the cumulative cost reaches the cap.

    APS_LIVE_MODEL=1 NEARAI_API_KEY=... python scripts/model_spike.py \
        --input rows.jsonl --out summary.json --cap-usd 2.0 \
        --model deepseek-ai/DeepSeek-V4.1-Flash-1017=0.30/1.20

Rows must come from the benchmark's PUBLIC split (or be synthetic); the
summary records row ids and verdicts, never row text. Keep inputs and
outputs outside the repository.
"""

import argparse
import json
import os
import pathlib
import statistics
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

import triage_model  # noqa: E402,F401  (side effect: makes aps_scanner importable)
from aps_scanner.engine_v2 import stage_b_request  # noqa: E402
from aps_scanner.envelope_v2 import from_benchmark_row  # noqa: E402
from aps_scanner.normalize import normalize_envelope_v2  # noqa: E402
from aps_scanner.report import compute_content_hash  # noqa: E402
from aps_scanner.stage_a import run_stage_a  # noqa: E402
from triage_model import config  # noqa: E402
from triage_model.backends import LiveBackend, RecordingBackend  # noqa: E402
from triage_model.client import ChatClient  # noqa: E402
from triage_model.judge import judge  # noqa: E402
from triage_model.schema import parse_output  # noqa: E402


class SpendCapReached(RuntimeError):
    pass


class MeasuringBackend:
    """Wraps a backend; records latency, usage, cost and first-reply validity."""

    def __init__(self, inner, price_in, price_out, ledger):
        self.inner = inner
        self.price_in = price_in
        self.price_out = price_out
        self.ledger = ledger
        self.calls = []

    def complete(self, messages, *, model_id, response_schema):
        if self.ledger["spent"] >= self.ledger["stop_at"]:
            raise SpendCapReached("spend cap reached")
        started = time.perf_counter()
        try:
            completion = self.inner.complete(
                messages, model_id=model_id, response_schema=response_schema
            )
        except Exception as err:
            self.calls.append(
                {"latency_s": time.perf_counter() - started, "error": type(err).__name__,
                 "error_text": str(err)[:80]}
            )
            raise
        latency = time.perf_counter() - started
        usage = completion.usage or {}
        prompt_tokens = int(usage.get("prompt_tokens") or 0)
        completion_tokens = int(usage.get("completion_tokens") or 0)
        cost = (prompt_tokens * self.price_in + completion_tokens * self.price_out) / 1e6
        self.ledger["spent"] += cost
        _value, errors = parse_output(completion.content)
        self.calls.append(
            {
                "latency_s": latency,
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "usage_present": bool(usage),
                "cost_usd": cost,
                "schema_ok": not errors,
            }
        )
        return completion


def percentile(values, fraction):
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, round(fraction * (len(ordered) - 1))))
    return ordered[index]


def summarize(model_id, price, calls, results):
    ok_calls = [c for c in calls if "error" not in c]
    latencies = [c["latency_s"] for c in ok_calls]
    prompt = [c["prompt_tokens"] for c in ok_calls]
    completion = [c["completion_tokens"] for c in ok_calls]
    costs = [c["cost_usd"] for c in ok_calls]
    statuses = [r["status"] for r in results]
    findings = len(results)
    return {
        "model_id": model_id,
        "price_usd_per_mtok": {"input": price[0], "output": price[1]},
        "findings": findings,
        "api_calls": len(calls),
        "api_errors": sorted({c["error"] + ": " + c["error_text"] for c in calls if "error" in c}),
        "replies_schema_valid": sum(1 for c in ok_calls if c["schema_ok"]),
        "replies_total": len(ok_calls),
        "findings_ok": statuses.count("ok"),
        "findings_fail_closed": {
            f: sum(1 for r in results if r.get("failure") == f)
            for f in sorted({r.get("failure") for r in results if r.get("failure")})
        },
        "latency_s": {
            "median": statistics.median(latencies) if latencies else None,
            "p95": percentile(latencies, 0.95),
            "max": max(latencies) if latencies else None,
        },
        "tokens_per_call": {
            "prompt_median": statistics.median(prompt) if prompt else None,
            "completion_median": statistics.median(completion) if completion else None,
            "completion_max": max(completion) if completion else None,
        },
        "usage_fields_present": all(c["usage_present"] for c in ok_calls) if ok_calls else None,
        "cost_usd": {
            "total": sum(costs),
            "per_call_mean": (sum(costs) / len(costs)) if costs else None,
            "per_finding_mean": (sum(costs) / findings) if findings else None,
        },
        "verdicts": [
            {
                "id": r["row_id"],
                "status": r["status"],
                "bountiable_probability": r.get("bountiable_probability"),
                "severity_call": r.get("severity_call"),
            }
            for r in results
        ],
    }


def parse_model(spec):
    model_id, _, price = spec.partition("=")
    price_in, _, price_out = price.partition("/")
    return model_id, (float(price_in), float(price_out))


def main(argv=None):
    parser = argparse.ArgumentParser(prog="model_spike")
    parser.add_argument("--input", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--model", action="append", required=True, help="ID=IN/OUT USD per Mtok")
    parser.add_argument("--cap-usd", type=float, required=True)
    parser.add_argument("--already-spent-usd", type=float, default=0.0)
    parser.add_argument("--stop-fraction", type=float, default=0.9)
    parser.add_argument("--response-format", default="json_schema")
    parser.add_argument("--max-tokens", type=int, default=1024)
    parser.add_argument("--record", help="optional cassette directory (outside the repo)")
    args = parser.parse_args(argv)
    if not config.live_enabled(os.environ):
        print("model_spike: needs APS_LIVE_MODEL=1 and NEARAI_API_KEY", file=sys.stderr)
        return 2
    rows = [
        json.loads(line)
        for line in pathlib.Path(args.input).read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    ledger = {"spent": args.already_spent_usd, "stop_at": args.cap_usd * args.stop_fraction}
    client = ChatClient(
        os.environ[config.API_KEY_ENV].strip(),
        base_url=config.base_url(os.environ),
        max_tokens=args.max_tokens,
        response_format=args.response_format,
    )
    summaries, stopped = [], False
    for spec in args.model:
        model_id, price = parse_model(spec)
        inner = LiveBackend(client)
        if args.record:
            inner = RecordingBackend(inner, args.record)
        backend = MeasuringBackend(inner, price[0], price[1], ledger)
        results = []
        for row in rows:
            envelope = from_benchmark_row(row)
            normalized = normalize_envelope_v2(envelope)
            stage_a = run_stage_a(normalized)
            request = stage_b_request(normalized, stage_a, compute_content_hash(normalized))
            try:
                result = judge(request, backend=backend, model_id=model_id)
            except SpendCapReached:
                stopped = True
                break
            results.append(dict(result, row_id=row["id"]))
        summaries.append(summarize(model_id, price, backend.calls, results))
        print(f"model_spike: {model_id}: {len(results)} findings, spent so far "
              f"{ledger['spent']:.4f} USD")
        if stopped:
            break
    report = {
        "response_format": args.response_format,
        "max_tokens": args.max_tokens,
        "rows": len(rows),
        "cap_usd": args.cap_usd,
        "spent_usd_total": ledger["spent"],
        "stopped_at_cap": stopped,
        "models": summaries,
    }
    pathlib.Path(args.out).write_text(json.dumps(report, indent=2, sort_keys=True), "utf-8")
    print(f"model_spike: wrote summary; total spent {ledger['spent']:.4f} USD")
    return 3 if stopped else 0


if __name__ == "__main__":
    sys.exit(main())
