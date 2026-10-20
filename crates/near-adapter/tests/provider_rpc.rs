//! End-to-end verification tests for the NEAR provider against a
//! deterministic stub JSON-RPC server (offline; loopback only).
//!
//! These tests exercise the full observation path: hint submitted, tx
//! fetched, facts re-verified, observation produced, reconciliation
//! deciding funding - the same code path the HTTP API uses. Canned
//! outcomes are built AFTER the intent exists, so the ft_transfer memo
//! always carries the real intent id, exactly as a payer's wallet would.

mod common;

use std::sync::{Arc, Mutex};

use aps_core::Amount;
use aps_near::{NearConfig, NearProvider, PaymentProvider, TxHint};
use aps_payment::{
    reconcile, AssetId, CreateIntent, DepositIntent, Observation, ProviderError, ReconcileOutcome,
};
use common::{successful_ft_transfer_tx, tx_not_found_error, StubRpc};

use serde_json::{json, Value};

const GATEWAY: &str = "gateway.near";
const ASSET: &str = "asset.near";
const SIGNER: &str = "payer.near";

fn near_config(url: String) -> NearConfig {
    NearConfig {
        rpc_url: url,
        network: "mainnet".to_string(),
        gateway_account_id: GATEWAY.to_string(),
        asset_contract_id: ASSET.to_string(),
        timeout_secs: 5,
    }
}

fn intent_request(amount: u128) -> CreateIntent {
    CreateIntent {
        asset_id: AssetId::new(ASSET),
        amount: Amount::from_atomic(amount),
        expires_at_unix: 10_000,
    }
}

/// The canned outcome cell: the handler serves the stored outcome for tx
/// requests, or a not-found error while the cell is empty.
type Cell = Arc<Mutex<Option<Value>>>;

fn start_stub() -> (StubRpc, Cell) {
    let cell: Cell = Arc::new(Mutex::new(None));
    let handler_cell = Arc::clone(&cell);
    let stub = StubRpc::start(Arc::new(move |request: &Value| {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match method {
            "tx" => {
                let stored = handler_cell.lock().map(|g| g.clone()).unwrap_or(None);
                match stored {
                    Some(outcome) => Ok(outcome),
                    None => Err(tx_not_found_error()),
                }
            }
            "status" => Ok(json!({
                "chain_id": "mainnet",
                "sync_info": { "latest_block_height": 100 }
            })),
            _ => Ok(Value::Null),
        }
    }));
    (stub, cell)
}

fn provider_with_intent(stub: &StubRpc) -> (NearProvider, DepositIntent) {
    let mut provider = NearProvider::new(near_config(stub.url.clone())).unwrap();
    provider.sync_clock(1_000);
    let intent = provider.create_intent(intent_request(1_000_000)).unwrap();
    (provider, intent)
}

fn store(cell: &Cell, outcome: Value) {
    *cell.lock().expect("cell lock") = Some(outcome);
}

fn submit_hint(provider: &mut NearProvider, intent: &DepositIntent) {
    provider
        .submit_hint(
            &intent.intent_id,
            TxHint {
                signer_id: SIGNER.to_string(),
                tx_hash: "txhash".to_string(),
            },
        )
        .unwrap();
}

fn ft_transfer(cell: &Cell, intent: &DepositIntent, amount: &str, deposit: &str) {
    store(
        cell,
        successful_ft_transfer_tx(
            "txhash",
            SIGNER,
            ASSET,
            GATEWAY,
            intent.intent_id.as_str(),
            amount,
            deposit,
        ),
    );
}

#[test]
fn verified_deposit_reconciles_to_fund_and_replays_stably() {
    let (stub, cell) = start_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    ft_transfer(&cell, &intent, "1000000", "1");
    submit_hint(&mut provider, &intent);

    let observation = provider.observe(&intent.intent_id).unwrap();
    let Observation::Confirmed {
        tx_ref,
        asset_id,
        amount,
    } = &observation
    else {
        panic!("expected a confirmed observation, got {observation:?}");
    };
    assert_eq!(tx_ref, "txhash");
    assert_eq!(asset_id.as_str(), ASSET);
    assert_eq!(amount.atomic(), 1_000_000);

    let outcome = reconcile(&intent, &observation);
    assert_eq!(
        outcome,
        ReconcileOutcome::Fund {
            tx_ref: "txhash".to_string()
        }
    );

    // Observing again replays the same confirmation forever.
    let replay = provider.observe(&intent.intent_id).unwrap();
    assert_eq!(replay, observation);
}

#[test]
fn amount_mismatch_produces_a_confirmed_mismatch_that_never_funds() {
    // The payer transferred 2 USDC where the intent asked for 1 USDC.
    let (stub, cell) = start_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    ft_transfer(&cell, &intent, "2000000", "1");
    submit_hint(&mut provider, &intent);
    let observation = provider.observe(&intent.intent_id).unwrap();
    let Observation::Confirmed { amount, .. } = &observation else {
        panic!("expected a confirmed observation, got {observation:?}");
    };
    assert_eq!(amount.atomic(), 2_000_000);
    let outcome = reconcile(&intent, &observation);
    assert!(matches!(outcome, ReconcileOutcome::Mismatch { .. }));
}

fn structural_error_test(outcome: Value, needle: &str) {
    let (stub, cell) = start_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    store(&cell, outcome);
    submit_hint(&mut provider, &intent);
    let error = provider
        .observe(&intent.intent_id)
        .expect_err("structural mismatches must be client errors");
    match error {
        ProviderError::Validation(message) => {
            assert!(
                message.contains(needle),
                "message {message:?} should contain {needle:?}"
            );
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
    // A structural mismatch never finalizes the intent: no observation
    // is recorded and no funds can move.
    assert!(provider.intent(&intent.intent_id).is_some());
}

#[test]
fn memo_mismatch_is_a_client_error() {
    let (stub, cell) = start_stub();
    let (_provider, _intent) = provider_with_intent(&stub);
    let outcome = successful_ft_transfer_tx(
        "txhash",
        SIGNER,
        ASSET,
        GATEWAY,
        "aps2_someone_elses_intent",
        "1000000",
        "1",
    );
    drop(cell);
    structural_error_test(outcome, "memo");
}

#[test]
fn wrong_recipient_is_a_client_error() {
    let (stub, cell) = start_stub();
    let (_provider, _intent) = provider_with_intent(&stub);
    drop(cell);
    let outcome = successful_ft_transfer_tx(
        "txhash",
        SIGNER,
        ASSET,
        "elsewhere.near",
        "memo",
        "1000000",
        "1",
    );
    structural_error_test(outcome, "gateway account");
}

#[test]
fn wrong_token_contract_is_a_client_error() {
    let (stub, cell) = start_stub();
    let (_provider, _intent) = provider_with_intent(&stub);
    drop(cell);
    let outcome = successful_ft_transfer_tx(
        "txhash",
        SIGNER,
        "other-token.near",
        GATEWAY,
        "memo",
        "1000000",
        "1",
    );
    structural_error_test(outcome, "calls contract");
}

#[test]
fn missing_one_yocto_deposit_is_a_client_error() {
    let (stub, cell) = start_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    // Correct memo, correct amount, but the 1-yoctoNEAR deposit missing.
    ft_transfer(&cell, &intent, "1000000", "0");
    submit_hint(&mut provider, &intent);
    let error = provider
        .observe(&intent.intent_id)
        .expect_err("a zero deposit must be a client error");
    match error {
        ProviderError::Validation(message) => {
            assert!(message.contains("yoctoNEAR"), "message: {message}");
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn malformed_amount_is_a_client_error() {
    let (stub, cell) = start_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    ft_transfer(&cell, &intent, "not-a-number", "1");
    submit_hint(&mut provider, &intent);
    let error = provider
        .observe(&intent.intent_id)
        .expect_err("a malformed amount must be a client error");
    match error {
        ProviderError::Validation(message) => {
            assert!(message.contains("integer"), "message: {message}");
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn no_ft_transfer_call_is_a_client_error() {
    let outcome = json!({
        "status": { "SuccessValue": "" },
        "transaction": {
            "signer_id": SIGNER,
            "receiver_id": ASSET,
            "actions": [ { "FunctionCall": {
                "method_name": "ft_total_supply",
                "args": "",
                "gas": 1u64,
                "deposit": "0"
            } } ]
        },
        "transaction_outcome": {},
        "receipts_outcome": []
    });
    structural_error_test(outcome, "no ft_transfer");
}

#[test]
fn failed_transaction_is_a_rejected_observation() {
    let (stub, cell) = start_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    store(&cell, common::failed_tx("txhash", SIGNER, ASSET));
    submit_hint(&mut provider, &intent);
    let observation = provider.observe(&intent.intent_id).unwrap();
    let outcome = reconcile(&intent, &observation);
    assert!(matches!(outcome, ReconcileOutcome::Rejected { .. }));
    // The rejection is replayed, not re-fetched.
    assert_eq!(provider.observe(&intent.intent_id).unwrap(), observation);
}

#[test]
fn not_found_transaction_stays_pending_until_it_appears() {
    let (stub, cell) = start_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    submit_hint(&mut provider, &intent);
    // The cell is empty: the node has not seen the transaction yet.
    assert_eq!(
        provider.observe(&intent.intent_id),
        Ok(Observation::Pending)
    );
    // The payer's wallet broadcast lands on chain; the cell is filled.
    ft_transfer(&cell, &intent, "1000000", "1");
    let observation = provider.observe(&intent.intent_id).unwrap();
    assert!(matches!(observation, Observation::Confirmed { .. }));
    // The request asked for FINAL finality, matching the documented API.
    let requests = stub.requests();
    let tx_request = requests
        .iter()
        .find(|r| r.get("method").and_then(Value::as_str) == Some("tx"))
        .expect("a tx request was made");
    assert_eq!(
        tx_request
            .get("params")
            .and_then(|p| p.get("wait_until"))
            .and_then(Value::as_str),
        Some("FINAL")
    );
}

#[test]
fn batched_transaction_with_the_matching_transfer_funds() {
    // A batch where the first action is unrelated and only the second is
    // the matching ft_transfer.
    use base64::Engine as _;
    let (stub, cell) = start_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    let args = json!({
        "receiver_id": GATEWAY,
        "amount": "1000000",
        "memo": intent.intent_id.as_str(),
    });
    let args_base64 = base64::engine::general_purpose::STANDARD.encode(args.to_string().as_bytes());
    let outcome = json!({
        "status": { "SuccessValue": "" },
        "transaction": {
            "signer_id": SIGNER,
            "receiver_id": ASSET,
            "actions": [
                { "FunctionCall": {
                    "method_name": "storage_deposit",
                    "args": "e30=",
                    "gas": 1u64,
                    "deposit": "10000000000000000000000"
                } },
                { "FunctionCall": {
                    "method_name": "ft_transfer",
                    "args": args_base64,
                    "gas": 30000000000000u64,
                    "deposit": "1"
                } }
            ]
        },
        "transaction_outcome": {},
        "receipts_outcome": []
    });
    store(&cell, outcome);
    submit_hint(&mut provider, &intent);
    let observation = provider.observe(&intent.intent_id).unwrap();
    assert!(matches!(observation, Observation::Confirmed { .. }));
    assert_eq!(
        reconcile(&intent, &observation),
        ReconcileOutcome::Fund {
            tx_ref: "txhash".to_string()
        }
    );
}
