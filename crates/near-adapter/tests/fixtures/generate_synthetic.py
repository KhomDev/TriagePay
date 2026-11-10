"""Generate the synthetic EXPERIMENTAL_tx_status fixtures for aps-near.

Every fixture mirrors the structure of the real mainnet capture
(mainnet_direct_ft_transfer.json): transaction, transaction_outcome,
receipts_outcome (executor, status, logs, receipt_ids) and receipts
(predecessor, receiver, Action body). Identifiers are clearly synthetic.
"""

import base64
import json
import pathlib
import sys

OUT = pathlib.Path(sys.argv[1])

ASSET = "usdc.fixture.testnet"
GATEWAY = "gateway.fixture.testnet"
PAYER = "payer.fixture.testnet"
AGENT = "agent.fixture.testnet"
WALLET = "wallet.fixture.testnet"
SPOOF = "spoof.fixture.testnet"
MEMO = "aps2_fixture_00001"
AMOUNT = "1000000"
PK = "ed25519:SyntheticFixtureKey1111111111111111111111111"
TGAS = 1_000_000_000_000


def b64(obj):
    return base64.b64encode(json.dumps(obj, separators=(",", ":")).encode()).decode()


def fc(method, args, gas_tgas, deposit):
    return {
        "FunctionCall": {
            "args": b64(args),
            "deposit": deposit,
            "gas": gas_tgas * TGAS,
            "method_name": method,
        }
    }


def event(old, new, amount, memo=None, std="nep141", name="ft_transfer"):
    entry = {"old_owner_id": old, "new_owner_id": new, "amount": amount}
    if memo is not None:
        entry["memo"] = memo
    body = {"standard": std, "version": "1.0.0", "event": name, "data": [entry]}
    return "EVENT_JSON:" + json.dumps(body, separators=(",", ":"))


def wallet_event(name, payment_id):
    body = {
        "standard": "policy_wallet",
        "version": "1.0.0",
        "event": name,
        "data": [{"payment_id": str(payment_id), "agent_id": AGENT, "token": ASSET,
                  "receiver_id": GATEWAY, "amount": AMOUNT}],
    }
    return "EVENT_JSON:" + json.dumps(body, separators=(",", ":"))


def body(rid, pred, recv, actions, signer, gas_price="1000000000"):
    return {
        "predecessor_id": pred,
        "priority": 0,
        "receipt": {
            "Action": {
                "actions": actions,
                "gas_price": gas_price,
                "input_data_ids": [],
                "is_promise_yield": False,
                "output_data_receivers": [],
                "signer_id": signer,
                "signer_public_key": PK,
            }
        },
        "receipt_id": rid,
        "receiver_id": recv,
    }


def refund_body(rid, to):
    return body(rid, "system", to, [{"Transfer": {"deposit": "1000000000000000000000"}}], to, "0")


def outcome(rid, executor, status, logs=(), children=()):
    return {
        "block_hash": "SyntheticBlock" + rid[-6:],
        "id": rid,
        "outcome": {
            "executor_id": executor,
            "gas_burnt": 2_400_000_000_000,
            "logs": list(logs),
            "metadata": {"gas_profile": [], "version": 3},
            "receipt_ids": list(children),
            "status": status,
            "tokens_burnt": "240000000000000000000",
        },
        "proof": [],
    }


def tx(tx_hash, signer, receiver, actions, first_receipt):
    return {
        "transaction": {
            "actions": actions,
            "hash": tx_hash,
            "nonce": 7,
            "priority_fee": 0,
            "public_key": PK,
            "receiver_id": receiver,
            "signature": "ed25519:SyntheticSignature",
            "signer_id": signer,
        },
        "transaction_outcome": {
            "block_hash": "SyntheticBlockTx",
            "id": tx_hash,
            "outcome": {
                "executor_id": signer,
                "gas_burnt": 300_000_000_000,
                "logs": [],
                "metadata": {"gas_profile": None, "version": 1},
                "receipt_ids": [first_receipt],
                "status": {"SuccessReceiptId": first_receipt},
                "tokens_burnt": "30000000000000000000",
            },
            "proof": [],
        },
    }


def ok(value=""):
    return {"SuccessValue": value}


def failure(message):
    return {"Failure": {"ActionError": {"index": 0, "kind": {"FunctionCallError": {
        "ExecutionError": message}}}}}


def write(name, description, expect, result, final="FINAL"):
    result = {"final_execution_status": final, **result}
    doc = {
        "_fixture": {
            "synthetic": True,
            "description": description,
            "expect": expect,
            "placeholders": {"asset": ASSET, "gateway": GATEWAY, "memo": MEMO},
            "shape_reference": "mainnet_direct_ft_transfer.json (real EXPERIMENTAL_tx_status capture)",
        },
        "response": {"jsonrpc": "2.0", "id": "aps-near", "result": result},
    }
    with open(OUT / name, "w", newline="\n", encoding="utf-8") as handle:
        json.dump(doc, handle, indent=2)
        handle.write("\n")


def direct(tx_hash, args, logs, status=None, top=None, extra_actions=(), final="FINAL",
           method="ft_transfer", deposit="1"):
    r1 = tx_hash + "-r1"
    r2 = tx_hash + "-r2"
    actions = list(extra_actions) + [fc(method, args, 30, deposit)]
    result = tx(tx_hash, PAYER, ASSET, actions, r1)
    status = status or ok("")
    result["status"] = top or status
    result["receipts_outcome"] = [
        outcome(r1, ASSET, status, logs, [r2]),
        outcome(r2, PAYER, ok(""), [], []),
    ]
    result["receipts"] = [body(r1, PAYER, ASSET, actions, PAYER), refund_body(r2, PAYER)]
    return result, final


def transfer_args(receiver=GATEWAY, amount=AMOUNT, memo=MEMO):
    return {"receiver_id": receiver, "amount": amount, "memo": memo}


def main():
    OUT.mkdir(parents=True, exist_ok=True)

    # 1. A wallet-originated transfer (policy-wallet `pay`).
    h = "SynthWalletPay"
    r = [f"{h}-r{i}" for i in range(1, 6)]
    pay_args = {"token": ASSET, "receiver_id": GATEWAY, "amount": AMOUNT, "memo": MEMO}
    result = tx(h, AGENT, WALLET, [fc("pay", pay_args, 100, "0")], r[0])
    result["status"] = ok("dHJ1ZQ==")
    result["receipts_outcome"] = [
        outcome(r[0], WALLET, {"SuccessReceiptId": r[2]}, [wallet_event("pay_requested", 0)], [r[1], r[2]]),
        outcome(r[1], ASSET, ok(""), [event(WALLET, GATEWAY, AMOUNT, MEMO)], [r[3]]),
        outcome(r[2], WALLET, ok("dHJ1ZQ=="), [wallet_event("pay_settled", 0)], [r[4]]),
        outcome(r[3], AGENT, ok(""), [], []),
        outcome(r[4], AGENT, ok(""), [], []),
    ]
    result["receipts"] = [
        body(r[0], AGENT, WALLET, [fc("pay", pay_args, 100, "0")], AGENT),
        body(r[1], WALLET, ASSET, [fc("ft_transfer", transfer_args(), 15, "1")], AGENT),
        body(r[2], WALLET, WALLET, [fc("on_pay_complete", {"agent_id": AGENT, "token": ASSET,
                                     "receiver_id": GATEWAY, "amount": AMOUNT, "payment_id": "0",
                                     "window_index": "0"}, 10, "0")], AGENT),
        refund_body(r[3], AGENT),
        refund_body(r[4], AGENT),
    ]
    write("synthetic_wallet_pay.json",
          "Policy-wallet pay: the agent calls wallet.pay, the wallet sends ft_transfer to the token "
          "(predecessor = wallet), the token logs the nep141 event, the wallet callback settles.",
          "Confirmed; payer (old_owner_id) and predecessor are the wallet", result)

    # 2. A failed direct transfer.
    res, fin = direct("SynthFailedTransfer", transfer_args(), [],
                      status=failure("Smart contract panicked: The account doesn't have enough balance"))
    write("synthetic_failed_transfer.json",
          "Direct ft_transfer that panicked in the token (insufficient balance); the final status is Failure.",
          "Failed", res, fin)

    # 3. Wallet pay whose token transfer failed while the callback succeeded.
    h = "SynthWalletPayTransferFailed"
    r = [f"{h}-r{i}" for i in range(1, 5)]
    result = tx(h, AGENT, WALLET, [fc("pay", pay_args, 100, "0")], r[0])
    result["status"] = ok("ZmFsc2U=")
    result["receipts_outcome"] = [
        outcome(r[0], WALLET, {"SuccessReceiptId": r[2]}, [wallet_event("pay_requested", 1)], [r[1], r[2]]),
        # The token logged a matching event and then panicked: the log line
        # survives in the outcome but the receipt status is Failure.
        outcome(r[1], ASSET, failure("Smart contract panicked: The account doesn't have enough balance"),
                [event(WALLET, GATEWAY, AMOUNT, MEMO)], []),
        outcome(r[2], WALLET, ok("ZmFsc2U="), [wallet_event("pay_failed", 1)], [r[3]]),
        outcome(r[3], AGENT, ok(""), [], []),
    ]
    result["receipts"] = [
        body(r[0], AGENT, WALLET, [fc("pay", pay_args, 100, "0")], AGENT),
        body(r[1], WALLET, ASSET, [fc("ft_transfer", transfer_args(), 15, "1")], AGENT),
        body(r[2], WALLET, WALLET, [fc("on_pay_complete", {"payment_id": "1"}, 10, "0")], AGENT),
        refund_body(r[3], AGENT),
    ]
    write("synthetic_wallet_pay_transfer_failed.json",
          "Policy-wallet pay where the token receipt failed after logging a matching event, while the "
          "wallet callback returned normally: top-level SuccessValue does not prove the transfer.",
          "Failed (token receipt failed); the failed receipt's logs are ignored", result)

    # 4. Not final yet.
    res, _ = direct("SynthNotFinal", transfer_args(), [event(PAYER, GATEWAY, AMOUNT, MEMO)])
    write("synthetic_not_final.json",
          "A successful direct transfer reported at EXECUTED_OPTIMISTIC, not FINAL.",
          "NotFinalYet", res, final="EXECUTED_OPTIMISTIC")

    # 5. Spoofed event from a non-token contract.
    h = "SynthSpoofedEvent"
    r = [f"{h}-r{i}" for i in range(1, 3)]
    result = tx(h, PAYER, SPOOF, [fc("pay", {"memo": MEMO}, 30, "0")], r[0])
    result["status"] = ok("")
    result["receipts_outcome"] = [
        outcome(r[0], SPOOF, ok(""), [event(PAYER, GATEWAY, AMOUNT, MEMO)], [r[1]]),
        outcome(r[1], PAYER, ok(""), [], []),
    ]
    result["receipts"] = [
        body(r[0], PAYER, SPOOF, [fc("pay", {"memo": MEMO}, 30, "0")], PAYER),
        refund_body(r[1], PAYER),
    ]
    write("synthetic_spoofed_event.json",
          "A non-token contract logs a perfect nep141 ft_transfer event to the gateway with the intent "
          "memo and amount; no token receipt exists.",
          "StructuralMismatch; the spoofed log is ignored", result)

    # 6. Spoofed event plus a real dust transfer from the token.
    h = "SynthSpoofedEventWithDust"
    r = [f"{h}-r{i}" for i in range(1, 4)]
    result = tx(h, PAYER, SPOOF, [fc("pay", {"memo": MEMO}, 60, "0")], r[0])
    result["status"] = ok("")
    result["receipts_outcome"] = [
        outcome(r[0], SPOOF, {"SuccessReceiptId": r[1]}, [event(PAYER, GATEWAY, AMOUNT, MEMO)], [r[1]]),
        outcome(r[1], ASSET, ok(""), [event(SPOOF, GATEWAY, "1", "dust")], [r[2]]),
        outcome(r[2], PAYER, ok(""), [], []),
    ]
    result["receipts"] = [
        body(r[0], PAYER, SPOOF, [fc("pay", {"memo": MEMO}, 60, "0")], PAYER),
        body(r[1], SPOOF, ASSET, [fc("ft_transfer", transfer_args(amount="1", memo="dust"), 15, "1")], PAYER),
        refund_body(r[2], PAYER),
    ]
    write("synthetic_spoofed_event_with_dust.json",
          "A non-token contract logs a fake matching event and also makes a real 1-unit ft_transfer to "
          "the gateway with an unrelated memo.",
          "StructuralMismatch; neither the fake event nor the dust transfer funds", result)

    # 7. ft_transfer_call refund case.
    h = "SynthTransferCallRefund"
    r = [f"{h}-r{i}" for i in range(1, 5)]
    call_args = {"receiver_id": GATEWAY, "amount": AMOUNT, "memo": MEMO, "msg": ""}
    result = tx(h, PAYER, ASSET, [fc("ft_transfer_call", call_args, 100, "1")], r[0])
    result["status"] = ok("IjAi")
    result["receipts_outcome"] = [
        outcome(r[0], ASSET, {"SuccessReceiptId": r[2]}, [event(PAYER, GATEWAY, AMOUNT, MEMO)], [r[1], r[2]]),
        outcome(r[1], GATEWAY, {"Failure": {"ActionError": {"index": 0, "kind": {"FunctionCallError": {
            "CompilationError": {"CodeDoesNotExist": {"account_id": GATEWAY}}}}}}}, [], []),
        outcome(r[2], ASSET, ok("IjAi"), [event(GATEWAY, PAYER, AMOUNT, "refund")], [r[3]]),
        outcome(r[3], PAYER, ok(""), [], []),
    ]
    result["receipts"] = [
        body(r[0], PAYER, ASSET, [fc("ft_transfer_call", call_args, 100, "1")], PAYER),
        body(r[1], ASSET, GATEWAY, [fc("ft_on_transfer", {"sender_id": PAYER, "amount": AMOUNT, "msg": ""}, 40, "0")], PAYER),
        body(r[2], ASSET, ASSET, [fc("ft_resolve_transfer", {"sender_id": PAYER, "receiver_id": GATEWAY, "amount": AMOUNT}, 10, "0")], PAYER),
        refund_body(r[3], PAYER),
    ]
    write("synthetic_ft_transfer_call_refund.json",
          "ft_transfer_call to a plain gateway account: the token logs the outgoing event with the intent "
          "memo, ft_on_transfer fails (no contract), ft_resolve_transfer refunds with a reverse event.",
          "StructuralMismatch (ft_transfer_call); never funds", result)

    # 8-10. Wrong memo, wrong amount, wrong recipient.
    res, fin = direct("SynthWrongMemo", transfer_args(memo="aps2_someone_else"),
                      [event(PAYER, GATEWAY, AMOUNT, "aps2_someone_else")])
    write("synthetic_wrong_memo.json", "Direct ft_transfer to the gateway with another intent's memo.",
          "StructuralMismatch (memo)", res, fin)
    res, fin = direct("SynthWrongAmount", transfer_args(amount="2000000"),
                      [event(PAYER, GATEWAY, "2000000", MEMO)])
    write("synthetic_wrong_amount.json", "Direct ft_transfer of 2000000 where the intent asks 1000000.",
          "Confirmed with amount 2000000; reconciliation Mismatch; recorded as an orphan", res, fin)
    res, fin = direct("SynthWrongRecipient", transfer_args(receiver="elsewhere.fixture.testnet"),
                      [event(PAYER, "elsewhere.fixture.testnet", AMOUNT, MEMO)])
    write("synthetic_wrong_recipient.json", "Direct ft_transfer with the intent memo to another account.",
          "StructuralMismatch (gateway account)", res, fin)

    # 11. Two matching transfers in separate token receipts.
    h = "SynthTwoTransfers"
    r = [f"{h}-r{i}" for i in range(1, 5)]
    result = tx(h, AGENT, WALLET, [fc("pay_twice", pay_args, 100, "0")], r[0])
    result["status"] = ok("")
    result["receipts_outcome"] = [
        outcome(r[0], WALLET, ok(""), [], [r[1], r[2]]),
        outcome(r[1], ASSET, ok(""), [event(WALLET, GATEWAY, AMOUNT, MEMO)], [r[3]]),
        outcome(r[2], ASSET, ok(""), [event(WALLET, GATEWAY, AMOUNT, MEMO)], []),
        outcome(r[3], AGENT, ok(""), [], []),
    ]
    result["receipts"] = [
        body(r[0], AGENT, WALLET, [fc("pay_twice", pay_args, 100, "0")], AGENT),
        body(r[1], WALLET, ASSET, [fc("ft_transfer", transfer_args(), 15, "1")], AGENT),
        body(r[2], WALLET, ASSET, [fc("ft_transfer", transfer_args(), 15, "1")], AGENT),
        refund_body(r[3], AGENT),
    ]
    write("synthetic_two_matching_transfers.json",
          "A contract sends two separate ft_transfer receipts to the gateway, both with the intent memo.",
          "Confirmed by the first token receipt; the second is an orphan", result)

    # 12. One token receipt carrying two matching events (batched ft_transfer actions).
    res, fin = direct("SynthOneReceiptTwoEvents", transfer_args(),
                      [event(PAYER, GATEWAY, AMOUNT, MEMO), event(PAYER, GATEWAY, AMOUNT, MEMO)],
                      extra_actions=[fc("ft_transfer", transfer_args(), 30, "1")])
    write("synthetic_one_receipt_two_events.json",
          "One batched receipt with two ft_transfer actions, both paying the intent.",
          "StructuralMismatch (ambiguous); both events recorded as orphans", res, fin)

    # 13. Batched storage_deposit + ft_transfer (a common payer pattern).
    res, fin = direct("SynthBatchStorageAndTransfer", transfer_args(),
                      ["Account gateway.fixture.testnet registered with storage deposit",
                       event(PAYER, GATEWAY, AMOUNT, MEMO)],
                      extra_actions=[fc("storage_deposit", {"account_id": GATEWAY}, 30,
                                        "1250000000000000000000")])
    write("synthetic_batch_storage_and_transfer.json",
          "One batched receipt: storage_deposit for the gateway, then the paying ft_transfer.",
          "Confirmed", res, fin)

    # 14. Top-level SuccessReceiptId (a shape the node never sends).
    res, fin = direct("SynthTopLevelReceiptId", transfer_args(), [event(PAYER, GATEWAY, AMOUNT, MEMO)],
                      top={"SuccessReceiptId": "SynthTopLevelReceiptId-r1"})
    write("synthetic_top_level_success_receipt_id.json",
          "A response whose top-level status is SuccessReceiptId, which is not a FinalExecutionStatus.",
          "StructuralMismatch (malformed status)", res, fin)


if __name__ == "__main__":
    main()
