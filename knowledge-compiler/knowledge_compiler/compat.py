"""Report-to-ruleset compatibility checking.

Old reports carry a ruleset id; this table says whether a report produced
under one ruleset can be compared with scans under another. Unknown ids
are incompatible by default (fail closed).
"""

KNOWN_RULESETS = {
    "rules-v0.synthetic.1": {
        "version": "0.1.0",
        "note": "synthetic baseline: the nine deterministic gates",
        "supersedes": None,
    },
    # A different engine (Stage A of triage v2), not a successor: v0 reports
    # are not comparable with v1 reports, so nothing supersedes anything.
    "rules-v1.public": {
        "version": "1.0.0",
        "note": "public generic Stage A gates mapped onto the public taxonomy",
        "supersedes": None,
    },
}


def check(report_ruleset_id, current_ruleset_id):
    """Return {'compatible': bool, 'note': str} for a report ruleset id
    against the current one."""
    report_entry = KNOWN_RULESETS.get(report_ruleset_id)
    current_entry = KNOWN_RULESETS.get(current_ruleset_id)
    if report_entry is None or current_entry is None:
        return {
            "compatible": False,
            "note": f"unknown ruleset id (report: {report_ruleset_id}, "
            f"current: {current_ruleset_id})",
        }
    if report_ruleset_id == current_ruleset_id:
        return {"compatible": True, "note": "identical rulesets"}
    # Walk the supersedes chain from the report forward.
    seen = {report_ruleset_id}
    step = report_entry.get("supersedes")
    while step:
        if step == current_ruleset_id:
            return {
                "compatible": True,
                "note": f"{current_ruleset_id} supersedes {report_ruleset_id}",
            }
        if step in seen:
            break
        seen.add(step)
        step = KNOWN_RULESETS.get(step, {}).get("supersedes")
    return {
        "compatible": False,
        "note": f"no compatibility path from {report_ruleset_id} to {current_ruleset_id}",
    }
