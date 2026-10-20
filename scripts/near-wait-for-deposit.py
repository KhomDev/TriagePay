#!/usr/bin/env python3
"""Wait for a USDC deposit to land on the configured NEAR account.

Polls the public fastnear indexer (read-only, no key) until a token whose
symbol is USDC (or a known USDC contract) appears with a non-zero balance,
then prints the contract id so NEAR_ASSET_CONTRACT can be set and the
near-demo flow can run.

Usage:
    python scripts/near-wait-for-deposit.py [--account ID] [--timeout-secs 600]

Defaults to NEAR_ACCOUNT_ID from the environment. Exit codes:
0 deposit found (contract printed), 1 timeout or account error.
"""

import argparse
import json
import os
import sys
import time
import urllib.request

KNOWN_USDC_CONTRACTS = (
    "a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.factory.bridge.near",  # USDC.e
    "usdc.tkn.prv.near",
)


def fetch_tokens(account):
    url = f"https://api.fastnear.com/v1/account/{account}/ft"
    with urllib.request.urlopen(url, timeout=15) as response:
        return json.loads(response.read().decode("utf-8")).get("tokens", [])


def is_usdc(entry):
    if entry.get("contract_id") in KNOWN_USDC_CONTRACTS:
        return True
    # fall back to a metadata probe
    import base64

    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": "m",
            "method": "query",
            "params": {
                "request_type": "call_function",
                "finality": "final",
                "account_id": entry["contract_id"],
                "method_name": "ft_metadata",
                "args_base64": base64.b64encode(b"{}").decode("ascii"),
            },
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        "https://rpc.mainnet.near.org",
        data=body,
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            result = json.loads(response.read().decode("utf-8"))["result"]
        meta = json.loads(bytes(result["result"]).decode("utf-8"))
        return "usdc" in (meta.get("symbol") or "").lower()
    except Exception:  # noqa: BLE001 - unknown contracts are simply skipped
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--account", default=os.environ.get("NEAR_ACCOUNT_ID", ""))
    parser.add_argument("--timeout-secs", type=int, default=600)
    args = parser.parse_args()
    if not args.account:
        print("no account configured (set NEAR_ACCOUNT_ID or --account)")
        return 1

    deadline = time.time() + args.timeout_secs
    print(f"waiting for a USDC deposit on {args.account} (timeout {args.timeout_secs}s)")
    attempt = 0
    while time.time() < deadline:
        attempt += 1
        try:
            tokens = fetch_tokens(args.account)
            for entry in tokens:
                balance = entry.get("balance", "0")
                if balance not in ("0", "") and is_usdc(entry):
                    contract = entry["contract_id"]
                    print(f"[FOUND] {balance} atomic units of {contract}")
                    print(f"set NEAR_ASSET_CONTRACT={contract}")
                    return 0
            others = ", ".join(
                f"{t['contract_id']}:{t.get('balance', '?')}" for t in tokens
            )
            print(f"  [{attempt}] no USDC yet; holdings: {others or '(none)'}")
        except Exception as err:  # noqa: BLE001
            print(f"  [{attempt}] indexer probe failed: {err}")
        time.sleep(10)
    print("timeout: no USDC deposit observed")
    return 1


if __name__ == "__main__":
    sys.exit(main())
