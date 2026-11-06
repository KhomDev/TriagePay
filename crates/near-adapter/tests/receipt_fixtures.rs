//! Receipt-level verifier tests against recorded EXPERIMENTAL_tx_status
//! responses (offline).
//!
//! `mainnet_direct_ft_transfer.json` is a real mainnet capture (public
//! chain data). Every `synthetic_*.json` fixture is labelled synthetic
//! inside the file and mirrors the real capture's structure; they are
//! produced by `fixtures/generate_synthetic.py`.
//!
//! Each acceptance condition of the verifier has a positive and a
//! negative case here:
//!
//! | condition | positive | negative |
//! |---|---|---|
//! | final, top-level SuccessValue | mainnet, wallet pay | not final, failed, top-level SuccessReceiptId |
//! | token-executed receipt with its own success | mainnet, wallet pay | spoofed event, wallet pay with failed transfer |
//! | FunctionCall ft_transfer with 1 yocto, not ft_transfer_call | mainnet, batch | ft_transfer_call refund, zero deposit |
//! | exactly one matching event, u128 amount | mainnet | wrong memo, wrong recipient, two events in one receipt, bad amount |
//! | at most one funding transfer per intent | wallet pay | two matching transfers |

mod common;

use aps_core::Amount;
use aps_near::rpc::parse_tx_outcome;
use aps_near::{check_outcome, ReceiptCheck, Verification};
use aps_payment::{AssetId, DepositIntent, IntentId, IntentStatus};
use serde_json::Value;

const FIXTURE_ASSET: &str = "usdc.fixture.testnet";
const FIXTURE_GATEWAY: &str = "gateway.fixture.testnet";
const FIXTURE_MEMO: &str = "aps2_fixture_00001";

const MAINNET_USDC: &str = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";
const MAINNET_GATEWAY: &str = "gateway.hlpok1lad.tg";
const MAINNET_MEMO: &str = "aps2_2160_6ad8a68a_00001";
const MAINNET_TX: &str = "ZonAdEjgAUM4tBnCR3nJTxe9LHiv7KcsbqRZdp9dns1";

fn intent(id: &str, gateway: &str, asset: &str, amount: u128) -> DepositIntent {
    DepositIntent {
        intent_id: IntentId::new(id.to_string()),
        deposit_address: gateway.to_string(),
        asset_id: AssetId::new(asset),
        amount: Amount::from_atomic(amount),
        expires_at_unix: 10_000,
        status: IntentStatus::Pending,
    }
}

fn fixture_intent() -> DepositIntent {
    intent(FIXTURE_MEMO, FIXTURE_GATEWAY, FIXTURE_ASSET, 1_000_000)
}

fn mainnet_intent() -> DepositIntent {
    intent(MAINNET_MEMO, MAINNET_GATEWAY, MAINNET_USDC, 100_000)
}

fn result_of(raw: &str) -> Value {
    common::fixture_result(raw, None)
}

fn check_value(intent: &DepositIntent, tx_hash: &str, result: &Value) -> ReceiptCheck {
    let outcome = parse_tx_outcome(tx_hash, result).expect("fixture parses");
    check_outcome(intent, &outcome)
}

fn check(intent: &DepositIntent, tx_hash: &str, raw: &str) -> ReceiptCheck {
    check_value(intent, tx_hash, &result_of(raw))
}

fn assert_mismatch(check: &ReceiptCheck, needle: &str) {
    match &check.verification {
        Verification::StructuralMismatch(message) => assert!(
            message.contains(needle),
            "message {message:?} should contain {needle:?}"
        ),
        other => panic!("expected a structural mismatch containing {needle:?}, got {other:?}"),
    }
    assert!(check.evidence.is_none());
}

const MAINNET: &str = include_str!("fixtures/mainnet_direct_ft_transfer.json");
const WALLET_PAY: &str = include_str!("fixtures/synthetic_wallet_pay.json");
const FAILED: &str = include_str!("fixtures/synthetic_failed_transfer.json");
const WALLET_PAY_FAILED: &str = include_str!("fixtures/synthetic_wallet_pay_transfer_failed.json");
const NOT_FINAL: &str = include_str!("fixtures/synthetic_not_final.json");
const SPOOFED: &str = include_str!("fixtures/synthetic_spoofed_event.json");
const SPOOFED_DUST: &str = include_str!("fixtures/synthetic_spoofed_event_with_dust.json");
const TRANSFER_CALL: &str = include_str!("fixtures/synthetic_ft_transfer_call_refund.json");
const WRONG_MEMO: &str = include_str!("fixtures/synthetic_wrong_memo.json");
const WRONG_AMOUNT: &str = include_str!("fixtures/synthetic_wrong_amount.json");
const WRONG_RECIPIENT: &str = include_str!("fixtures/synthetic_wrong_recipient.json");
const TWO_TRANSFERS: &str = include_str!("fixtures/synthetic_two_matching_transfers.json");
const ONE_RECEIPT_TWO: &str = include_str!("fixtures/synthetic_one_receipt_two_events.json");
const BATCH: &str = include_str!("fixtures/synthetic_batch_storage_and_transfer.json");
const TOP_RECEIPT_ID: &str = include_str!("fixtures/synthetic_top_level_success_receipt_id.json");

#[test]
fn every_synthetic_fixture_is_labelled_synthetic() {
    for raw in [
        WALLET_PAY,
        FAILED,
        WALLET_PAY_FAILED,
        NOT_FINAL,
        SPOOFED,
        SPOOFED_DUST,
        TRANSFER_CALL,
        WRONG_MEMO,
        WRONG_AMOUNT,
        WRONG_RECIPIENT,
        TWO_TRANSFERS,
        ONE_RECEIPT_TWO,
        BATCH,
        TOP_RECEIPT_ID,
    ] {
        let doc: Value = serde_json::from_str(raw).expect("JSON");
        assert_eq!(doc["_fixture"]["synthetic"], true);
        assert!(doc["_fixture"]["description"].is_string());
    }
    let real: Value = serde_json::from_str(MAINNET).expect("JSON");
    assert_eq!(real["_fixture"]["synthetic"], false);
    assert_eq!(real["_fixture"]["network"], "mainnet");
}

// ----- the real mainnet capture -------------------------------------------

#[test]
fn mainnet_direct_transfer_confirms_with_receipt_evidence() {
    let check = check(&mainnet_intent(), MAINNET_TX, MAINNET);
    assert_eq!(
        check.verification,
        Verification::Confirmed {
            tx_ref: MAINNET_TX.to_string(),
            amount_atomic: "100000".to_string(),
        }
    );
    let evidence = check.evidence.expect("evidence");
    assert_eq!(
        evidence.receipt_id,
        "HcfgQShNoZY1czCKBLdK9TzUxXPCV5rcp93kB6efWPf2"
    );
    assert_eq!(evidence.executor_id, MAINNET_USDC);
    assert_eq!(evidence.predecessor_id, "hlpok1lad.tg");
    assert_eq!(evidence.payer_id, "hlpok1lad.tg");
    assert_eq!(evidence.receiver_id, MAINNET_GATEWAY);
    assert_eq!(evidence.memo, MAINNET_MEMO);
    assert!(check.orphans.is_empty());
}

#[test]
fn mainnet_transfer_does_not_fund_another_intent_gateway_or_asset() {
    let other_memo = intent(
        "aps2_2160_6ad8a68a_00002",
        MAINNET_GATEWAY,
        MAINNET_USDC,
        100_000,
    );
    assert_mismatch(&check(&other_memo, MAINNET_TX, MAINNET), "memo");
    let other_gateway = intent(MAINNET_MEMO, "other.near", MAINNET_USDC, 100_000);
    assert_mismatch(
        &check(&other_gateway, MAINNET_TX, MAINNET),
        "gateway account",
    );
    let other_asset = intent(
        MAINNET_MEMO,
        MAINNET_GATEWAY,
        "usdt.tether-token.near",
        100_000,
    );
    assert_mismatch(&check(&other_asset, MAINNET_TX, MAINNET), "asset contract");
}

#[test]
fn mainnet_transfer_without_receipt_bodies_cannot_confirm() {
    // The plain `tx` method returns this shape: no receipts array.
    let mut result = result_of(MAINNET);
    result.as_object_mut().expect("object").remove("receipts");
    assert_mismatch(
        &check_value(&mainnet_intent(), MAINNET_TX, &result),
        "receipt bodies",
    );
}

#[test]
fn mainnet_transfer_with_zero_deposit_body_is_rejected() {
    let mut result = result_of(MAINNET);
    result["receipts"][0]["receipt"]["Action"]["actions"][0]["FunctionCall"]["deposit"] =
        Value::String("0".to_string());
    assert_mismatch(
        &check_value(&mainnet_intent(), MAINNET_TX, &result),
        "yoctoNEAR",
    );
}

#[test]
fn mainnet_transfer_with_unparseable_amount_is_rejected() {
    let mut result = result_of(MAINNET);
    let log = result["receipts_outcome"][0]["outcome"]["logs"][0]
        .as_str()
        .expect("log line")
        .replace("\"amount\":\"100000\"", "\"amount\":\"1e5\"");
    result["receipts_outcome"][0]["outcome"]["logs"][0] = Value::String(log);
    assert_mismatch(
        &check_value(&mainnet_intent(), MAINNET_TX, &result),
        "integer",
    );
}

// ----- condition 1: finality and the top-level status ----------------------

#[test]
fn not_final_yet_is_pending() {
    let check = check(&fixture_intent(), "SynthNotFinal", NOT_FINAL);
    assert_eq!(check.verification, Verification::NotFinalYet);
    assert!(check.evidence.is_none());
}

#[test]
fn failed_transfer_is_failed() {
    let check = check(&fixture_intent(), "SynthFailedTransfer", FAILED);
    assert!(matches!(check.verification, Verification::Failed(_)));
    assert!(check.evidence.is_none());
}

#[test]
fn top_level_success_receipt_id_is_malformed_not_success() {
    let check = check(&fixture_intent(), "SynthTopLevelReceiptId", TOP_RECEIPT_ID);
    assert_mismatch(&check, "malformed");
}

#[test]
fn top_level_failure_turns_a_landed_transfer_into_an_orphan() {
    // A matching token transfer landed, but the transaction's final status
    // is Failure (for example a failing callback after the transfer).
    let mut result = result_of(WALLET_PAY);
    result["status"] = serde_json::json!({ "Failure": { "ActionError": { "index": 0 } } });
    let check = check_value(&fixture_intent(), "SynthWalletPay", &result);
    assert!(matches!(check.verification, Verification::Failed(_)));
    assert!(check.evidence.is_none());
    assert_eq!(check.orphans.len(), 1);
}

// ----- condition 2: executor binding (spoof resistance) --------------------

#[test]
fn wallet_originated_transfer_confirms_with_the_wallet_as_payer() {
    let check = check(&fixture_intent(), "SynthWalletPay", WALLET_PAY);
    assert!(matches!(
        check.verification,
        Verification::Confirmed { ref amount_atomic, .. } if amount_atomic == "1000000"
    ));
    let evidence = check.evidence.expect("evidence");
    assert_eq!(evidence.executor_id, FIXTURE_ASSET);
    assert_eq!(evidence.predecessor_id, "wallet.fixture.testnet");
    assert_eq!(evidence.payer_id, "wallet.fixture.testnet");
    assert_eq!(evidence.receipt_id, "SynthWalletPay-r2");
}

#[test]
fn spoofed_event_from_a_non_token_contract_never_confirms() {
    let check = check(&fixture_intent(), "SynthSpoofedEvent", SPOOFED);
    assert_mismatch(&check, "asset contract");
    assert!(check.orphans.is_empty());
}

#[test]
fn spoofed_event_next_to_a_real_dust_transfer_never_confirms() {
    let check = check(&fixture_intent(), "SynthSpoofedEventWithDust", SPOOFED_DUST);
    // The only token-executed event carries an unrelated memo; the fake
    // matching event logged by the spoof contract is ignored.
    assert_mismatch(&check, "memo");
    assert!(check.orphans.is_empty());
}

#[test]
fn spoof_contract_named_like_the_asset_prefix_is_still_foreign() {
    // An executor id that merely contains the asset id is not the asset.
    let mut result = result_of(SPOOFED);
    result["receipts_outcome"][0]["outcome"]["executor_id"] =
        Value::String(format!("{FIXTURE_ASSET}.evil.testnet"));
    assert_mismatch(
        &check_value(&fixture_intent(), "SynthSpoofedEvent", &result),
        "asset contract",
    );
}

#[test]
fn failed_token_receipt_logs_are_ignored_even_under_top_level_success() {
    let check = check(
        &fixture_intent(),
        "SynthWalletPayTransferFailed",
        WALLET_PAY_FAILED,
    );
    match &check.verification {
        Verification::Failed(reason) => assert!(reason.contains("token transfer receipt failed")),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(check.evidence.is_none());
    assert!(check.orphans.is_empty());
}

// ----- condition 3: method binding -----------------------------------------

#[test]
fn ft_transfer_call_refund_case_never_confirms() {
    let check = check(&fixture_intent(), "SynthTransferCallRefund", TRANSFER_CALL);
    assert_mismatch(&check, "ft_transfer_call");
}

#[test]
fn batched_storage_deposit_and_transfer_confirms() {
    let check = check(&fixture_intent(), "SynthBatchStorageAndTransfer", BATCH);
    assert!(matches!(check.verification, Verification::Confirmed { .. }));
    assert_eq!(
        check.evidence.expect("evidence").payer_id,
        "payer.fixture.testnet"
    );
}

// ----- condition 4: the event itself ---------------------------------------

#[test]
fn wrong_memo_wrong_recipient_and_wrong_amount() {
    assert_mismatch(
        &check(&fixture_intent(), "SynthWrongMemo", WRONG_MEMO),
        "memo",
    );
    assert_mismatch(
        &check(&fixture_intent(), "SynthWrongRecipient", WRONG_RECIPIENT),
        "gateway account",
    );
    // A wrong amount is a confirmed transfer of a different amount: the
    // pure reconciliation turns it into a mismatch that never funds.
    let wrong_amount = check(&fixture_intent(), "SynthWrongAmount", WRONG_AMOUNT);
    assert!(matches!(
        wrong_amount.verification,
        Verification::Confirmed { ref amount_atomic, .. } if amount_atomic == "2000000"
    ));
}

#[test]
fn two_matching_events_in_one_receipt_are_ambiguous_orphans() {
    let check = check(
        &fixture_intent(),
        "SynthOneReceiptTwoEvents",
        ONE_RECEIPT_TWO,
    );
    assert_mismatch(&check, "more than one matching transfer event");
    assert_eq!(check.orphans.len(), 2);
}

// ----- at most one funding transfer per intent -----------------------------

#[test]
fn a_second_matching_transfer_is_an_orphan_never_extra_credit() {
    let check = check(&fixture_intent(), "SynthTwoTransfers", TWO_TRANSFERS);
    assert_eq!(
        check.verification,
        Verification::Confirmed {
            tx_ref: "SynthTwoTransfers".to_string(),
            amount_atomic: "1000000".to_string(),
        }
    );
    assert_eq!(
        check.evidence.expect("evidence").receipt_id,
        "SynthTwoTransfers-r2"
    );
    assert_eq!(check.orphans.len(), 1);
    assert_eq!(check.orphans[0].receipt_id, "SynthTwoTransfers-r3");
    assert_eq!(check.orphans[0].amount_atomic, "1000000");
}

#[test]
fn unreachable_receipts_are_not_considered() {
    // A receipt outcome that is not reachable from the transaction's root
    // receipts (a malformed or spliced response) never funds.
    let mut result = result_of(WALLET_PAY);
    result["transaction_outcome"]["outcome"]["receipt_ids"] =
        serde_json::json!(["SynthWalletPay-r4"]);
    let check = check_value(&fixture_intent(), "SynthWalletPay", &result);
    assert_mismatch(&check, "asset contract");
}
