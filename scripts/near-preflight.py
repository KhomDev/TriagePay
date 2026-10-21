#!/usr/bin/env python3
"""Read-only NEAR preflight for AgentPay Sentinel.

Checks, without signing or mutating anything: RPC connectivity, the
wallet account (and its NEAR balance for gas), the access key type, the
settlement asset's metadata, and the wallet's asset balance. Run this
before any mainnet end-to-end flow. Never prints key material.

Usage:
    python scripts/near-preflight.py [--account ID] [--rpc URL]
        [--network mainnet|testnet] [--asset CONTRACT] [--public-key KEY]

Values default to the NEAR_* environment variables (see .env.example).
Exit codes: 0 all checks passed, 1 a check failed, 2 RPC unusable.
"""

import argparse
import json
import os
import sys
import urllib.request

# Native USDC on mainnet: the settlement asset (docs/integrations/near.md).
# Bridged USDC.e is a legacy token and is not a default anywhere.
USDC_NATIVE_MAINNET = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1"


def rpc_call(url, method, params):
    body = json.dumps(
        {"jsonrpc": "2.0", "id": "preflight", "method": method, "params": params}
    ).encode("utf-8")
    request = urllib.request.Request(
        url, data=body, headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(request, timeout=20) as response:
        payload = json.loads(response.read().decode("utf-8"))
    if "error" in payload:
        raise RuntimeError(
            f"RPC error {payload['error'].get('code')}: "
            f"{payload['error'].get('message')} {payload['error'].get('data', '')}"
        )
    return payload["result"]


def call_contract(url, contract, method, args):
    args_b64 = json.dumps(args).encode("utf-8")
    import base64

    result = rpc_call(
        url,
        "query",
        {
            "request_type": "call_function",
            "finality": "final",
            "account_id": contract,
            "method_name": method,
            "args_base64": base64.b64encode(args_b64).decode("ascii"),
        },
    )
    return json.loads(bytes(result["result"]).decode("utf-8"))


def yocto_to_near(yocto):
    try:
        return int(yocto) / 10**24
    except (TypeError, ValueError):
        return 0.0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--account", default=os.environ.get("NEAR_ACCOUNT_ID", ""))
    parser.add_argument("--public-key", default=os.environ.get("NEAR_PUBLIC_KEY", ""))
    parser.add_argument(
        "--network", default=os.environ.get("NEAR_NETWORK", "mainnet")
    )
    parser.add_argument(
        "--rpc",
        default=os.environ.get("NEAR_RPC_URL", ""),
    )
    parser.add_argument(
        "--asset",
        default=os.environ.get("NEAR_ASSET_CONTRACT", USDC_NATIVE_MAINNET),
    )
    args = parser.parse_args()

    url = args.rpc or (
        "https://rpc.testnet.near.org" if args.network == "testnet"
        else "https://rpc.mainnet.near.org"
    )
    if not url.startswith("http"):
        url = "https://" + url

    # Some privacy endpoints (for example 1RPC) do not expose the NEAR
    # JSON-RPC surface even though they proxy other chains. Probe first;
    # fall back to the official public endpoint with a clear notice.
    official = (
        "https://rpc.testnet.near.org" if args.network == "testnet"
        else "https://rpc.mainnet.near.org"
    )
    try:
        probe = rpc_call(url, "status", [])
        print(f"[ok] RPC endpoint answered (chain_id={probe['chain_id']})")
    except Exception:  # noqa: BLE001
        if url != official:
            print(f"[warn] configured endpoint did not answer the NEAR JSON-RPC "
                  f"status call; falling back to the official public endpoint")
            url = official
    failures = []

    try:
        status = rpc_call(url, "status", [])
        print(f"[ok] RPC {args.network}: chain_id={status['chain_id']} "
              f"height={status.get('sync_info', {}).get('latest_block_height')}")
    except Exception as err:  # noqa: BLE001 - diagnostics are the point
        print(f"[FAIL] RPC unreachable: {err}")
        return 2

    if not args.account:
        print("[skip] no account configured (set NEAR_ACCOUNT_ID or --account)")
        return 1

    try:
        account = rpc_call(
            url, "query",
            {"request_type": "view_account", "finality": "final",
             "account_id": args.account},
        )
        near_balance = yocto_to_near(account.get("amount"))
        print(f"[ok] account {args.account}: NEAR balance={near_balance:.6f} "
              f"(gas needs >= ~0.05)")
        if near_balance < 0.05:
            failures.append("NEAR balance below a safe gas floor")
    except Exception as err:  # noqa: BLE001
        print(f"[FAIL] account {args.account}: {err}")
        failures.append("account missing or unreadable")

    if args.public_key:
        try:
            key = rpc_call(
                url, "query",
                {"request_type": "view_access_key", "finality": "final",
                 "account_id": args.account, "public_key": args.public_key},
            )
            permission = key.get("permission")
            if permission == "FullAccess":
                print(f"[ok] access key on account: FullAccess (nonce "
                      f"{key.get('nonce')}) - can sign transfers")
            else:
                print(f"[warn] access key is a function-call key "
                      f"({json.dumps(permission)}) - it cannot send the deposit "
                      f"ft_transfer: NEP-141 requires exactly 1 yoctoNEAR attached "
                      f"and function-call keys cannot attach a deposit. Use a "
                      f"FullAccess key for the payer, or the planned spend-policy "
                      f"wallet (docs/integrations/near.md, 'Security-track angle')")
        except Exception as err:  # noqa: BLE001
            print(f"[FAIL] access key not found on the account: {err}")
            failures.append("public key not registered on the account")

    try:
        metadata = call_contract(url, args.asset, "ft_metadata", {})
        print(f"[ok] asset {args.asset}: {metadata.get('name')} "
              f"({metadata.get('symbol')}), decimals={metadata.get('decimals')}")
        balance = call_contract(
            url, args.asset, "ft_balance_of", {"account_id": args.account}
        )
        decimals = int(metadata.get("decimals", 0))
        print(f"[ok] asset balance: {balance} atomic units "
              f"({int(balance) / 10**decimals:.6f} {metadata.get('symbol')})")
    except Exception as err:  # noqa: BLE001
        print(f"[FAIL] asset contract {args.asset}: {err}")
        failures.append("asset unreadable")

    if failures:
        print("\nverdict: NOT READY - " + "; ".join(failures))
        return 1
    print("\nverdict: READY - preflight passed; see scripts/near-demo.ps1")
    return 0


if __name__ == "__main__":
    sys.exit(main())
