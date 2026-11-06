#!/usr/bin/env python3
"""Provisioning plan for the policy wallet (docs/contracts/mainnet-deployment-plan.md).

By default this prints the exact owner-signed provisioning plan for mainnet:
accounts, wasm hash and size, storage stake (size x storage_amount_per_byte),
gas, maximum cost, and the initial policy. It never signs and never sends a
mainnet transaction; mainnet execution happens by hand, after the operator
approves the printed plan.

Testnet execution happens only with `--network testnet --execute`. It
delegates every signature to scripts/testnet/wallet-e2e.mjs (near-api-js),
which creates its own throwaway accounts (the account arguments here only
shape the printed plan), keeps their keys outside the repository and runs
the full end-to-end sequence. This script never handles key material.

Usage (stdlib only, run from anywhere inside the repository):
    python scripts/wallet-provision.py                       # mainnet plan
    python scripts/wallet-provision.py --owner OWNER --agent AGENT --gateway GW
    python scripts/wallet-provision.py --json                # plan as JSON
    python scripts/wallet-provision.py --live-storage-price  # read-only RPC read
    python scripts/wallet-provision.py --network testnet     # testnet plan only
    python scripts/wallet-provision.py --network testnet --execute
Exit codes: 0 plan printed or run finished, 1 run failed, 2 usage error or
refused, 3 the testnet faucet refused (see wallet-e2e.mjs).
"""

import argparse
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys
import urllib.request

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_WASM = REPO_ROOT / "contracts" / "target" / "near" / "policy_wallet" / "policy_wallet.wasm"
E2E_SCRIPT = REPO_ROOT / "scripts" / "testnet" / "wallet-e2e.mjs"

YOCTO_PER_NEAR = 10**24
TGAS = 10**12
# Live value read from EXPERIMENTAL_protocol_config on 2026-10-21 (mainnet and
# testnet); --live-storage-price re-reads it.
STORAGE_PRICE_PER_BYTE = 10**19
# Planning price for prepaid gas (unused gas is refunded down to the burn price).
PREPAID_YOCTO_PER_GAS = 10**9
# Live fee config 2026-10-21: deploy cost per byte, send plus execution.
DEPLOY_GAS_PER_BYTE = 6_812_999 + 64_572_944
ACCOUNT_RECORD_BYTES = 100
STATE_ESTIMATE_BYTES = 500
STAKE_HEADROOM_YOCTO = 4 * 10**22  # at least 0.04 NEAR, then rounded up to 0.01 NEAR
USDC_STORAGE_MIN_YOCTO = 1_250_000_000_000_000_000_000  # 0.00125 NEAR
# A deployed wasm is public; these byte patterns mean a host path leaked into it.
HOST_PATH_PATTERN = re.compile(rb"[A-Za-z]:\\|/Users/|/home/")

NETWORKS = {
    "mainnet": {
        "rpc": "https://rpc.mainnet.near.org",
        "token": "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
        "token_label": "native USDC (6 decimals)",
        "per_call": "50000",
        "daily": "200000",
    },
    "testnet": {
        "rpc": "https://rpc.testnet.near.org",
        "token": "wrap.testnet",
        "token_label": "wNEAR (24 decimals)",
        "per_call": "1000000000000000000000",
        "daily": "3000000000000000000000",
    },
}


def near(yocto):
    """Format yoctoNEAR as a decimal NEAR string without floats."""
    whole, frac = divmod(int(yocto), YOCTO_PER_NEAR)
    text = f"{frac:024d}".rstrip("0")
    return f"{whole}.{text}" if text else str(whole)


def gas_cost(gas):
    return gas * PREPAID_YOCTO_PER_GAS


def ceil_to(value, step):
    return -(-value // step) * step


def live_storage_price(rpc):
    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": "provision",
            "method": "EXPERIMENTAL_protocol_config",
            "params": {"finality": "final"},
        }
    ).encode()
    request = urllib.request.Request(rpc, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=20) as response:
        result = json.load(response)["result"]
    return int(result["runtime_config"]["storage_amount_per_byte"]), result.get("protocol_version")


def build_plan(args, wasm_bytes, sha256_hex, price, price_source):
    net = NETWORKS[args.network]
    owner = args.owner or "<operator>"
    agent = args.agent or "<agent-account>"
    gateway = args.gateway or "<gateway>"
    wallet = args.wallet_account or f"agent-wallet.{owner}"
    token = args.token or net["token"]
    per_call = args.per_call or net["per_call"]
    daily = args.daily or net["daily"]
    allowance = int(args.allowance_yocto)

    code_stake = wasm_bytes * price
    total_bytes = wasm_bytes + ACCOUNT_RECORD_BYTES + STATE_ESTIMATE_BYTES
    stake = total_bytes * price
    transfer = ceil_to(stake + STAKE_HEADROOM_YOCTO, 10**22)  # round up to 0.01 NEAR
    deploy_gas = wasm_bytes * DEPLOY_GAS_PER_BYTE

    def tx(tid, signer, receiver, actions, deposit, gas, note=""):
        return {
            "id": tid,
            "signer": signer,
            "receiver": receiver,
            "actions": actions,
            "deposit_yocto": str(deposit),
            "deposit_near": near(deposit),
            "gas": gas,
            "gas_tgas": round(gas / TGAS, 3),
            "max_cost_near": near(deposit + gas_cost(gas)),
            "note": note,
        }

    transactions = [
        tx(
            "T1",
            owner,
            wallet,
            [
                "CreateAccount",
                f"Transfer {near(transfer)} NEAR",
                "AddKey <K_deploy> (temporary full-access key, generated for this run, "
                "held outside the repository)",
            ],
            transfer,
            int(8.4 * TGAS),
            "create the wallet subaccount; the transfer covers the storage stake plus headroom",
        ),
        tx(
            "T2",
            wallet,
            wallet,
            [
                f"DeployContract policy_wallet.wasm ({wasm_bytes} bytes, SHA-256 {sha256_hex})",
                "FunctionCall new "
                + json.dumps({"owner": owner, "tokens": [token]})
                + ", deposit 0, gas 30 TGas",
                "DeleteKey <K_deploy> (the batch deletes its own signing key)",
            ],
            0,
            deploy_gas + int(30.6 * TGAS),
            "deploy, initialize (new is #[private]) and lock in one batch; "
            "afterwards the wallet has no access keys",
        ),
        tx(
            "T3",
            owner,
            token,
            [
                "FunctionCall storage_deposit "
                + json.dumps({"account_id": wallet, "registration_only": True})
            ],
            USDC_STORAGE_MIN_YOCTO,
            30 * TGAS,
            "register the wallet on the token; repeat for any payee not yet registered "
            "(check storage_balance_of first)",
        ),
        tx(
            "T4",
            owner,
            token,
            [
                "FunctionCall ft_transfer_call "
                + json.dumps({"receiver_id": wallet, "amount": "<funding>", "msg": ""})
            ],
            1,
            100 * TGAS,
            "fund the wallet; ft_on_transfer accepts the full amount and emits funded",
        ),
        tx(
            "T5",
            owner,
            wallet,
            [
                "FunctionCall set_agent "
                + json.dumps(
                    {
                        "agent_id": agent,
                        "per_call_max": per_call,
                        "daily_max": daily,
                        "payees": [gateway],
                    }
                )
            ],
            1,
            30 * TGAS,
            "register the agent policy (exactly 1 yocto, owner only)",
        ),
        tx(
            "T6",
            agent,
            agent,
            [
                f"AddKey <K_agent> function-call: receiver_id={wallet}, "
                f'method_names=["pay"], allowance={near(allowance)} NEAR'
            ],
            0,
            int(0.5 * TGAS),
            "the agent's pay-only key; the agent account must hold at least the allowance in NEAR",
        ),
    ]
    later = [
        tx(
            "T7",
            agent + " with <K_agent>",
            wallet,
            [
                "FunctionCall pay "
                + json.dumps(
                    {"token": token, "receiver_id": gateway, "amount": per_call, "memo": "<job id>"}
                )
            ],
            0,
            50 * TGAS,
            "first use (prompt 09); zero deposit, so a function-call key can sign it",
        ),
        tx(
            "T8",
            owner,
            wallet,
            [
                "FunctionCall pause {} (1 yocto)",
                "FunctionCall withdraw {token, amount: <full balance>, to: <owner>} "
                "per held token (1 yocto)",
                "FunctionCall close " + json.dumps({"beneficiary": owner}) + " (1 yocto)",
            ],
            1,
            50 * TGAS,
            "retirement, later: close deletes the account to the beneficiary only when "
            "every allowlisted balance is zero",
        ),
    ]
    setup_total = sum(int(t["deposit_yocto"]) + gas_cost(t["gas"]) for t in transactions)
    return {
        "plan": "policy-wallet provisioning",
        "network": args.network,
        "rpc": net["rpc"],
        "executes": False if args.network == "mainnet" else bool(args.execute),
        "accounts": {"owner": owner, "wallet": wallet, "agent": agent, "payees": [gateway]},
        "token": {"account_id": token, "label": net["token_label"]},
        "initial_policy": {"per_call_max": per_call, "daily_max": daily, "payees": [gateway]},
        "wasm": {
            "file": "contracts/target/near/policy_wallet/policy_wallet.wasm",
            "bytes": wasm_bytes,
            "sha256": sha256_hex,
        },
        "storage": {
            "storage_amount_per_byte_yocto": str(price),
            "price_source": price_source,
            "code_stake_near": near(code_stake),
            "bytes_with_account_and_state_estimate": total_bytes,
            "stake_near": near(stake),
            "wallet_transfer_near": near(transfer),
        },
        "gas_planning_price": "0.001 NEAR per TGas prepaid (unused gas is refunded)",
        "agent_key_allowance_near": near(allowance),
        "transactions": transactions,
        "later": later,
        "max_setup_cost_near": near(setup_total),
        "funding": "<funding> token units (operator choice), excluded from the NEAR total",
    }


def render(plan):
    policy, wasm, storage = plan["initial_policy"], plan["wasm"], plan["storage"]
    lines = [
        f"POLICY-WALLET PROVISIONING PLAN ({plan['network']})",
        "PLAN ONLY: this script never signs on mainnet. Each mainnet transaction runs only",
        "after the operator approves this exact plan.",
        "",
        f"Owner:   {plan['accounts']['owner']}",
        f"Wallet:  {plan['accounts']['wallet']}",
        f"Agent:   {plan['accounts']['agent']}",
        f"Payees:  {', '.join(plan['accounts']['payees'])}",
        f"Token:   {plan['token']['account_id']} ({plan['token']['label']})",
        f"Policy:  per_call_max {policy['per_call_max']}, daily_max {policy['daily_max']}",
        f"Wasm:    {wasm['file']}, {wasm['bytes']} bytes, SHA-256 {wasm['sha256']}",
        f"Storage: {storage['storage_amount_per_byte_yocto']} yocto/byte "
        f"({storage['price_source']})",
        f"         code stake {storage['code_stake_near']} NEAR; with account record and state "
        f"estimate {storage['bytes_with_account_and_state_estimate']} bytes = "
        f"{storage['stake_near']} NEAR",
        f"         wallet transfer {storage['wallet_transfer_near']} NEAR",
        f"Gas:     {plan['gas_planning_price']}",
        "",
    ]
    for group, title in (("transactions", "Setup"), ("later", "Later (not part of setup)")):
        lines.append(f"{title}:")
        for t in plan[group]:
            lines.append(f"  {t['id']}  signer {t['signer']}  ->  receiver {t['receiver']}")
            for action in t["actions"]:
                lines.append(f"        - {action}")
            lines.append(
                f"        deposit {t['deposit_near']} NEAR, gas {t['gas_tgas']:g} TGas, "
                f"max cost {t['max_cost_near']} NEAR"
            )
            if t["note"]:
                lines.append(f"        ({t['note']})")
        lines.append("")
    lines.append(
        f"Maximum setup cost T1-T6: {plan['max_setup_cost_near']} NEAR plus {plan['funding']}"
    )
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--network", choices=sorted(NETWORKS), default="mainnet")
    parser.add_argument(
        "--execute", action="store_true", help="testnet only: run the end-to-end sequence"
    )
    parser.add_argument("--wasm", default=str(DEFAULT_WASM))
    parser.add_argument("--owner")
    parser.add_argument("--agent")
    parser.add_argument("--gateway", help="the single payee on the initial allowlist")
    parser.add_argument("--wallet-account")
    parser.add_argument("--token")
    parser.add_argument("--per-call")
    parser.add_argument("--daily")
    parser.add_argument(
        "--allowance-yocto",
        default=str(5 * 10**23),
        help="agent key gas allowance (default 0.5 NEAR)",
    )
    parser.add_argument(
        "--live-storage-price",
        action="store_true",
        help="read storage_amount_per_byte over read-only RPC",
    )
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--keys-dir", help="testnet execute: off-repository keys directory")
    parser.add_argument("--evidence", help="testnet execute: off-repository evidence file")
    args = parser.parse_args(argv)

    if args.execute and args.network != "testnet":
        print("refused: --execute runs on testnet only; mainnet is plan-only", file=sys.stderr)
        return 2

    wasm_path = pathlib.Path(args.wasm)
    if not wasm_path.is_file():
        print(
            f"wasm not found at {args.wasm}; build it first (docs/contracts/build.md)",
            file=sys.stderr,
        )
        return 2
    data = wasm_path.read_bytes()
    sha256_hex = hashlib.sha256(data).hexdigest()
    if HOST_PATH_PATTERN.search(data):
        print(
            "refused: the wasm embeds a host path; rebuild it with the path remapping in "
            "docs/contracts/build.md",
            file=sys.stderr,
        )
        return 2

    price, price_source = (
        STORAGE_PRICE_PER_BYTE,
        "recorded 2026-10-21 from EXPERIMENTAL_protocol_config",
    )
    if args.live_storage_price:
        rpc = NETWORKS[args.network]["rpc"]
        price, protocol = live_storage_price(rpc)
        price_source = f"live read-only EXPERIMENTAL_protocol_config on {rpc}, protocol {protocol}"

    plan = build_plan(args, len(data), sha256_hex, price, price_source)
    text = json.dumps(plan, indent=2) + "\n" if args.json else render(plan)
    sys.stdout.write(text)
    print(f"plan sha256: {hashlib.sha256(text.encode()).hexdigest()}")

    if not args.execute:
        return 0
    node = shutil.which("node")
    if not node:
        print("node is required for --execute (scripts/testnet/wallet-e2e.mjs)", file=sys.stderr)
        return 2
    command = [
        node,
        str(E2E_SCRIPT),
        "--network",
        "testnet",
        "--wasm",
        str(wasm_path),
        "--expected-sha256",
        sha256_hex,
    ]
    if args.keys_dir:
        command += ["--keys-dir", args.keys_dir]
    if args.evidence:
        command += ["--evidence", args.evidence]
    print("executing the testnet end-to-end run through scripts/testnet/wallet-e2e.mjs", flush=True)
    return subprocess.call(command)


if __name__ == "__main__":
    sys.exit(main())
