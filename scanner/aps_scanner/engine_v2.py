"""Triage engine v2: Stage A gates, optional Stage B judge, combiner, report.

``triage(envelope)`` accepts a v1 or v2 envelope (v1 is mapped into v2
deterministically), runs Stage A, optionally asks a Stage B judge, and
returns a TriageReport whose public JSON is leakage-guarded.

Stage B is any callable ``judge(request: dict) -> dict``; the scanner never
imports a model client itself (the triage-model package provides one).
Whatever the judge returns is re-validated here; anything malformed, and
any exception, becomes a fail-closed Stage B result, which the combiner
treats as needs_evidence. The judge sees the injection flag but the flag
never changes a gate.
"""

import dataclasses
import math
import re
from dataclasses import dataclass, field

from . import combiner
from .envelope_v2 import FindingEnvelopeV2, from_v1
from .normalize import normalize_envelope_v2
from .public_guard import PublicGuardError, is_clean
from .report import compute_content_hash
from .schema import FindingEnvelope
from .stage_a import run_stage_a
from .taxonomy import is_public_category, ordered

REPORT_VERSION = 2
SEVERITY_CALLS = ("invalid", "info/low", "medium", "high")
RATIONALE_MAX = 600
EVIDENCE_CHECK = "format-only"
RATIONALE_WITHHELD = ""

_HEX64 = re.compile(r"[0-9a-f]{64}")
_ID = re.compile(r"[a-z][a-z0-9-]{1,48}")
_MODEL_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}")


class ScanInputErrorV2(ValueError):
    def __init__(self, errors):
        super().__init__("; ".join(errors) if errors else "invalid envelope")
        self.errors = list(errors)


def _not_run():
    return {"status": combiner.STAGE_B_NOT_RUN}


def _fail_closed(failure, base=None):
    result = {"status": combiner.STAGE_B_FAIL_CLOSED, "failure": failure}
    for key in ("model_id", "prompt_hash"):
        if base and isinstance(base.get(key), str):
            result[key] = base[key]
    return result


def sanitize_stage_b(result) -> dict:
    """Re-validate a judge's result; anything off becomes fail-closed."""
    if not isinstance(result, dict):
        return _fail_closed("malformed_stage_b")
    status = result.get("status")
    if status not in combiner.STAGE_B_STATUSES:
        return _fail_closed("malformed_stage_b")
    meta = {}
    model_id = result.get("model_id")
    if model_id is not None:
        if not isinstance(model_id, str) or not _MODEL_ID.fullmatch(model_id):
            return _fail_closed("malformed_stage_b")
        meta["model_id"] = model_id
    prompt_hash = result.get("prompt_hash")
    if prompt_hash is not None:
        if not isinstance(prompt_hash, str) or not _HEX64.fullmatch(prompt_hash):
            return _fail_closed("malformed_stage_b")
        meta["prompt_hash"] = prompt_hash
    verifiable = result.get("model_verifiable")
    if verifiable is not None:
        if not isinstance(verifiable, bool):
            return _fail_closed("malformed_stage_b")
        meta["model_verifiable"] = verifiable
    if status != combiner.STAGE_B_OK:
        failure = result.get("failure")
        clean = {"status": status, **meta}
        if isinstance(failure, str) and _ID.fullmatch(failure.replace("_", "-")):
            clean["failure"] = failure
        return clean
    probability = result.get("bountiable_probability")
    if (
        isinstance(probability, bool)
        or not isinstance(probability, (int, float))
        or not math.isfinite(probability)
        or not 0 <= probability <= 1
    ):
        return _fail_closed("malformed_stage_b", meta)
    severity = result.get("severity_call")
    hits = result.get("taxonomy_hits")
    missing = result.get("missing_evidence")
    rationale = result.get("public_rationale")
    digest = result.get("response_digest")
    if (
        severity not in SEVERITY_CALLS
        or not isinstance(hits, list)
        or not all(is_public_category(hit) for hit in hits)
        or not isinstance(missing, list)
        or not all(isinstance(m, str) and _ID.fullmatch(m) for m in missing)
        or not isinstance(rationale, str)
        or len(rationale) > RATIONALE_MAX
        or not isinstance(digest, str)
        or not _HEX64.fullmatch(digest)
        or "model_id" not in meta
        or "prompt_hash" not in meta
    ):
        return _fail_closed("malformed_stage_b", meta)
    return {
        "status": combiner.STAGE_B_OK,
        "bountiable_probability": round(float(probability), 6),
        "severity_call": severity,
        "taxonomy_hits": ordered(hits),
        "missing_evidence": sorted(set(missing)),
        "public_rationale": rationale,
        "response_digest": digest,
        **meta,
    }


def stage_b_request(normalized, stage_a, content_hash) -> dict:
    """What the judge sees: the normalized finding, Stage A's public
    categories, the injection flag, and the cache-key hashes."""
    finding = dataclasses.asdict(normalized)
    finding.pop("contains_embedded_instructions", None)
    return {
        "finding": finding,
        "stage_a_taxonomy_hits": list(stage_a.taxonomy_hits),
        "contains_embedded_instructions": normalized.contains_embedded_instructions,
        "envelope_hash": content_hash,
        "ruleset_hash": stage_a.ruleset_hash,
    }


@dataclass
class TriageReport:
    decision: str
    confidence_band: str
    stage_a: object
    stage_b: dict
    stage_b_decision: str | None
    content_hash: str
    contains_embedded_instructions: bool
    thresholds: combiner.Thresholds
    score: float | None
    rationale_withheld: bool = False
    taxonomy_hits: list = field(default_factory=list)

    def to_public_json(self) -> dict:
        a = self.stage_a
        b = self.stage_b
        stage_b_public = {
            "status": b["status"],
            "decision": self.stage_b_decision,
            "bountiable_probability": b.get("bountiable_probability"),
            "severity_call": b.get("severity_call"),
            "taxonomy_hits": list(b.get("taxonomy_hits", [])),
            "missing_evidence": list(b.get("missing_evidence", [])),
            "public_rationale": b.get("public_rationale"),
            "rationale_withheld": self.rationale_withheld,
            "response_digest": b.get("response_digest"),
            "failure": b.get("failure"),
        }
        return {
            "report_version": REPORT_VERSION,
            "decision": self.decision,
            "confidence_band": self.confidence_band,
            "missing_evidence": list(a.missing_evidence),
            "failed_gates": list(a.failed_gates),
            "held_gates": list(a.held_gates),
            "gate_results": [
                {
                    "gate": r.gate,
                    "status": r.status.value,
                    "reason": r.reason,
                    "category": r.category,
                }
                for r in a.gate_results
            ],
            "ruleset_version": a.ruleset_version,
            "ruleset_hash": a.ruleset_hash,
            "content_hash": self.content_hash,
            "contains_embedded_instructions": self.contains_embedded_instructions,
            "stage_a": {
                "decision": a.decision,
                "taxonomy_hits": list(a.taxonomy_hits),
                "penalty_factor": a.penalty_factor,
            },
            "stage_b": stage_b_public,
            "taxonomy_hits": list(self.taxonomy_hits),
            "model_id": b.get("model_id"),
            # The catalogue's TEE-verifiable flag for model_id (None when no
            # model ran or the judge did not report it).
            "model_verifiable": b.get("model_verifiable"),
            "prompt_hash": b.get("prompt_hash"),
            "thresholds": self.thresholds.public(),
            "score": self.score,
            "evidence_check": EVIDENCE_CHECK,
        }


def _guard(report: TriageReport) -> TriageReport:
    """Leakage-scan the free-text fields. A leaking rationale is withheld;
    any other leak (ruleset reasons, ids) refuses the report."""
    rationale = report.stage_b.get("public_rationale")
    if rationale and not is_clean(rationale):
        report.stage_b = dict(report.stage_b, public_rationale=RATIONALE_WITHHELD)
        report.rationale_withheld = True
    texts = [r.reason for r in report.stage_a.gate_results]
    texts += [r.gate for r in report.stage_a.gate_results]
    texts += list(report.stage_b.get("missing_evidence", []))
    texts.append(report.stage_b.get("model_id") or "")
    texts.append(report.stage_b.get("failure") or "")
    if not is_clean("\n".join(texts)):
        raise PublicGuardError("a public report field failed the leakage guard")
    return report


def triage(
    envelope,
    *,
    ruleset=None,
    judge=None,
    thresholds: combiner.Thresholds = combiner.DEFAULT_THRESHOLDS,
) -> TriageReport:
    """Triage one finding. Same input, same judge output, same report."""
    if isinstance(envelope, FindingEnvelope) and not isinstance(envelope, FindingEnvelopeV2):
        envelope = from_v1(envelope)
    errors = envelope.validate()
    if errors:
        raise ScanInputErrorV2(errors)
    normalized = normalize_envelope_v2(envelope)
    content_hash = compute_content_hash(normalized)
    stage_a = run_stage_a(normalized, ruleset)

    if judge is None:
        stage_b = _not_run()
    else:
        try:
            raw = judge(stage_b_request(normalized, stage_a, content_hash))
        except Exception:  # the judge is untrusted: any failure fails closed
            raw = _fail_closed("judge_error")
        stage_b = sanitize_stage_b(raw)

    b_decision = combiner.stage_b_decision(
        stage_b["status"], stage_b.get("bountiable_probability"), thresholds
    )
    final = combiner.combine(stage_a.decision, b_decision)
    band = combiner.confidence_band(final, stage_a.decision, stage_b["status"], b_decision)
    score = combiner.benchmark_score(
        stage_b["status"], stage_b.get("bountiable_probability"), stage_a.penalty_factor
    )
    report = TriageReport(
        decision=final,
        confidence_band=band,
        stage_a=stage_a,
        stage_b=stage_b,
        stage_b_decision=b_decision,
        content_hash=content_hash,
        contains_embedded_instructions=normalized.contains_embedded_instructions,
        thresholds=thresholds,
        score=score,
        taxonomy_hits=ordered(list(stage_a.taxonomy_hits) + stage_b.get("taxonomy_hits", [])),
    )
    return _guard(report)
