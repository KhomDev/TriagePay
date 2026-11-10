"""Smoke test of a running TriagePay API (stdlib only).

Full mode walks the same flow as scripts/demo.sh: a payment intent funded
by the mock provider, a reservation, a scan that settles against the
report hash, an invalid envelope rejected with 400, a refund, and the
final balances with conservation. Read-only mode only issues GET requests
and is the mode to use against a public URL.

Usage:
  python3 deploy/smoke.py [--base http://127.0.0.1:8080] [--read-only]
  docker compose exec -T aps python3 - < deploy/smoke.py     (in-container)
Exit codes: 0 all checks passed, 1 a check failed, 2 the API is unreachable.
"""

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request

ENVELOPE = {
    "title": "Rounding drain in vault share accounting",
    "claimed_impact": "Permanent loss of user funds via inflated share ratio",
    "mechanism": "First depositor inflates the share price before the pool opens",
    "attacker_capability": "Any external address with collateral",
    "preconditions": "Vault freshly deployed with empty reserves",
    "component_class": "vault",
    "scope_statement": "Vault deposit and mint path is in scope for this review",
    "deployed_state_evidence": "Vault live at 0x1234567890abcdef since block 5100",
    "public_rules_snapshot": "https://archive.example.org/rules/v1",
    "known_issue_declaration": None,
    "reproduction_summary": "Step 1: mint one share. Step 2: donate collateral. Step 3: redeem.",
    "evidence_refs": ["https://example.test/issue-family-1"],
    "code_excerpt": "function mint() external returns (uint256) { ... }",
}


class Smoke:
    def __init__(self, base):
        self.base = base.rstrip("/")
        self.failures = 0

    def call(self, method, path, body=None):
        data = None
        headers = {}
        if body is not None:
            data = body.encode("utf-8") if isinstance(body, str) else json.dumps(body).encode()
            headers["content-type"] = "application/json"
        req = urllib.request.Request(self.base + path, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=60) as res:
                return res.status, json.loads(res.read() or b"null")
        except urllib.error.HTTPError as err:
            raw = err.read()
            try:
                return err.code, json.loads(raw)
            except ValueError:
                return err.code, None

    def check(self, name, ok, detail=""):
        print(f"{'PASS' if ok else 'FAIL'} {name}{': ' + detail if detail else ''}")
        if not ok:
            self.failures += 1
        return ok


def wait_for_health(smoke, seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            status, body = smoke.call("GET", "/health")
            if status == 200 and body == {"status": "ok"}:
                return True
        except (urllib.error.URLError, OSError):
            pass
        time.sleep(1)
    return False


def full_flow(smoke):
    run = str(int(time.time()))
    account = f"smoke-{run}"
    status, intent = smoke.call(
        "POST",
        "/payment/intents",
        {"account_id": account, "asset_id": "asset.usdc.test", "amount_atomic": "5000000",
         "expires_in_secs": 3600},
    )
    if not smoke.check("create intent", status == 201, f"HTTP {status}"):
        return
    status, observed = smoke.call("POST", f"/payment/intents/{intent['intent_id']}/observations")
    smoke.check("mock provider funds the account",
                status == 200 and observed.get("funded") is True, f"HTTP {status}")

    status, res = smoke.call("POST", "/reservations", {
        "account_id": account, "sku": "scan.v1", "amount_atomic": "1000000",
        "idempotency_key": f"smoke-r1-{run}"})
    if not smoke.check("reserve scan.v1", status == 201, f"HTTP {status}"):
        return
    job = res["job_id"]
    status, _ = smoke.call("POST", f"/jobs/{job}/start")
    smoke.check("start job", status == 200, f"HTTP {status}")
    status, scan = smoke.call("POST", f"/jobs/{job}/scan", json.dumps(ENVELOPE))
    settled = status == 201 and scan["settlement"]["report_hash_hex"] == scan["report"][
        "content_hash"]
    smoke.check("scan settles against the report hash", settled, f"HTTP {status}")
    if status == 201:
        smoke.check("verdict", scan["report"]["decision"] == "supported",
                    scan["report"]["decision"])

    status, res2 = smoke.call("POST", "/reservations", {
        "account_id": account, "sku": "scan.v1", "amount_atomic": "1000000",
        "idempotency_key": f"smoke-r2-{run}"})
    job2 = res2["job_id"] if status == 201 else None
    if job2:
        smoke.call("POST", f"/jobs/{job2}/start")
        bad = dict(ENVELOPE, title="x" * 201)
        status, _ = smoke.call("POST", f"/jobs/{job2}/scan", json.dumps(bad))
        smoke.check("invalid envelope is a 400 and never charges", status == 400, f"HTTP {status}")
        status, _ = smoke.call("POST", f"/jobs/{job2}/refunds", {"reason": "smoke: refund"})
        smoke.check("refund the untouched reservation", status == 200, f"HTTP {status}")
        status, again = smoke.call("POST", f"/jobs/{job2}/refunds", {"reason": "smoke: refund"})
        smoke.check("a second refund replays", status == 200 and again.get("replayed") is True)

    status, bal = smoke.call("GET", f"/accounts/{account}")
    if smoke.check("read balances", status == 200, f"HTTP {status}"):
        atoms = {k: int(v["atomic"]) for k, v in bal.items()}
        smoke.check("balances", atoms["available"] == 4_000_000 and atoms["settled_total"]
                    == 1_000_000, json.dumps(atoms))
        smoke.check("conservation", atoms["deposited_total"] == atoms["available"]
                    + atoms["reserved"] + atoms["settled_total"])


def read_only(smoke):
    status, _ = smoke.call("GET", "/accounts/smoke-account-that-does-not-exist")
    smoke.check("unknown account is a typed 404", status == 404, f"HTTP {status}")
    status, _ = smoke.call("GET", "/jobs/job_ffffffffffffffff")
    smoke.check("unknown job is a typed 404", status == 404, f"HTTP {status}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    default = f"http://127.0.0.1:{os.environ.get('APS_PORT', '8080')}"
    parser.add_argument("--base", default=default)
    parser.add_argument("--read-only", action="store_true")
    parser.add_argument("--wait", type=int, default=30, help="seconds to wait for /health")
    args = parser.parse_args()
    smoke = Smoke(args.base)
    if not wait_for_health(smoke, args.wait):
        print(f"FAIL health: {args.base}/health did not answer", file=sys.stderr)
        return 2
    smoke.check("health", True)
    read_only(smoke)
    if not args.read_only:
        full_flow(smoke)
    print(f"smoke: {smoke.failures} failure(s)")
    return 1 if smoke.failures else 0


if __name__ == "__main__":
    sys.exit(main())
