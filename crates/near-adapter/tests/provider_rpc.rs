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
            m if common::is_tx_status_method(m) => {
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
    structural_error_test(outcome, "asset contract");
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
    let outcome = common::transfer_outcome(
        "txhash",
        SIGNER,
        ASSET,
        vec![json!({ "FunctionCall": {
            "method_name": "ft_total_supply",
            "args": "",
            "gas": 1u64,
            "deposit": "0"
        } })],
        json!({ "SuccessValue": "" }),
        Vec::new(),
    );
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
    // Observing the same hint again reports the same rejection.
    assert_eq!(provider.observe(&intent.intent_id).unwrap(), observation);
    // The rejection belongs to that hint, not to the intent: nothing binds
    // a failed transaction to an intent, so the intent stays open and a
    // later hint for the real payment funds it.
    assert!(matches!(
        provider.intent(&intent.intent_id).map(|i| &i.status),
        Some(aps_payment::IntentStatus::Pending)
    ));
    store(
        &cell,
        successful_ft_transfer_tx(
            "txreal",
            SIGNER,
            ASSET,
            GATEWAY,
            intent.intent_id.as_str(),
            "1000000",
            "1",
        ),
    );
    provider
        .submit_hint(
            &intent.intent_id,
            TxHint {
                signer_id: SIGNER.to_string(),
                tx_hash: "txreal".to_string(),
            },
        )
        .unwrap();
    let funded = provider.observe(&intent.intent_id).unwrap();
    assert_eq!(
        reconcile(&intent, &funded),
        ReconcileOutcome::Fund {
            tx_ref: "txreal".to_string()
        }
    );
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
    // The payer's broadcast lands on chain; the cell is filled.
    ft_transfer(&cell, &intent, "1000000", "1");
    let observation = provider.observe(&intent.intent_id).unwrap();
    assert!(matches!(observation, Observation::Confirmed { .. }));
    // Every status request asked for FINAL finality through a method that
    // returns receipt bodies.
    let requests = stub.requests();
    let tx_requests: Vec<&Value> = requests
        .iter()
        .filter(|r| {
            r.get("method")
                .and_then(Value::as_str)
                .is_some_and(common::is_tx_status_method)
        })
        .collect();
    assert!(!tx_requests.is_empty());
    for request in tx_requests {
        assert_ne!(
            request["method"], "tx",
            "the plain tx method omits receipts"
        );
        assert_eq!(request["params"]["wait_until"], "FINAL");
    }
}

#[test]
fn batched_transaction_with_the_matching_transfer_funds() {
    // A batch where the first action is unrelated (storage_deposit) and
    // the second is the matching ft_transfer, executed in one receipt.
    use base64::Engine as _;
    let (stub, cell) = start_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    let args = json!({
        "receiver_id": GATEWAY,
        "amount": "1000000",
        "memo": intent.intent_id.as_str(),
    });
    let args_base64 = base64::engine::general_purpose::STANDARD.encode(args.to_string().as_bytes());
    let outcome = common::transfer_outcome(
        "txhash",
        SIGNER,
        ASSET,
        vec![
            json!({ "FunctionCall": {
                "method_name": "storage_deposit",
                "args": "e30=",
                "gas": 1u64,
                "deposit": "10000000000000000000000"
            } }),
            json!({ "FunctionCall": {
                "method_name": "ft_transfer",
                "args": args_base64,
                "gas": 30000000000000u64,
                "deposit": "1"
            } }),
        ],
        json!({ "SuccessValue": "" }),
        vec![common::nep141_event(
            SIGNER,
            GATEWAY,
            "1000000",
            intent.intent_id.as_str(),
        )],
    );
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

/// A stub that answers tx_status with METHOD_NOT_FOUND (nearcore 2.13)
/// and serves the outcome only on EXPERIMENTAL_tx_status.
fn start_legacy_stub() -> (StubRpc, Cell) {
    let cell: Cell = Arc::new(Mutex::new(None));
    let handler_cell = Arc::clone(&cell);
    let stub = StubRpc::start(Arc::new(move |request: &Value| {
        match request.get("method").and_then(Value::as_str) {
            Some("EXPERIMENTAL_tx_status") => {
                let stored = handler_cell.lock().map(|g| g.clone()).unwrap_or(None);
                stored.ok_or_else(tx_not_found_error)
            }
            Some(method) => Err(common::method_not_found_error(method)),
            None => Ok(Value::Null),
        }
    }));
    (stub, cell)
}

#[test]
fn tx_status_falls_back_to_experimental_once_and_remembers() {
    let (stub, cell) = start_legacy_stub();
    let (mut provider, intent) = provider_with_intent(&stub);
    submit_hint(&mut provider, &intent);
    assert_eq!(
        provider.observe(&intent.intent_id),
        Ok(Observation::Pending)
    );
    ft_transfer(&cell, &intent, "1000000", "1");
    assert!(matches!(
        provider.observe(&intent.intent_id),
        Ok(Observation::Confirmed { .. })
    ));
    let methods: Vec<String> = stub
        .requests()
        .iter()
        .filter_map(|r| r.get("method").and_then(Value::as_str).map(str::to_string))
        .collect();
    // tx_status is tried exactly once; after METHOD_NOT_FOUND every lookup
    // goes straight to EXPERIMENTAL_tx_status.
    assert_eq!(
        methods,
        vec![
            "tx_status".to_string(),
            "EXPERIMENTAL_tx_status".to_string(),
            "EXPERIMENTAL_tx_status".to_string()
        ]
    );
}

#[test]
fn rpc_timeout_is_not_final_yet_never_failure() {
    let stub = StubRpc::start(Arc::new(|_request: &Value| Err(common::timeout_error())));
    let (mut provider, intent) = provider_with_intent(&stub);
    submit_hint(&mut provider, &intent);
    assert_eq!(
        provider.observe(&intent.intent_id),
        Ok(Observation::Pending)
    );
    assert_eq!(
        provider.intent(&intent.intent_id).map(|i| &i.status),
        Some(&aps_payment::IntentStatus::Pending)
    );
}

#[test]
fn other_rpc_errors_are_provider_unavailable() {
    let stub = StubRpc::start(Arc::new(|_request: &Value| {
        Err(json!({ "code": -32000, "message": "Server error", "data": "internal" }))
    }));
    let (mut provider, intent) = provider_with_intent(&stub);
    submit_hint(&mut provider, &intent);
    assert_eq!(
        provider.observe(&intent.intent_id),
        Err(ProviderError::ProviderUnavailable)
    );
}

const FIXTURE_ASSET: &str = "usdc.fixture.testnet";
const FIXTURE_GATEWAY: &str = "gateway.fixture.testnet";

fn fixture_provider(stub: &StubRpc) -> (NearProvider, DepositIntent) {
    let config = NearConfig {
        rpc_url: stub.url.clone(),
        network: "testnet".to_string(),
        gateway_account_id: FIXTURE_GATEWAY.to_string(),
        asset_contract_id: FIXTURE_ASSET.to_string(),
        timeout_secs: 5,
    };
    let mut provider = NearProvider::new(config).unwrap();
    provider.sync_clock(1_000);
    let intent = provider
        .create_intent(CreateIntent {
            asset_id: AssetId::new(FIXTURE_ASSET),
            amount: Amount::from_atomic(1_000_000),
            expires_at_unix: 10_000,
        })
        .unwrap();
    (provider, intent)
}

fn fixture_hint(provider: &mut NearProvider, intent: &DepositIntent, signer: &str, tx: &str) {
    provider
        .submit_hint(
            &intent.intent_id,
            TxHint {
                signer_id: signer.to_string(),
                tx_hash: tx.to_string(),
            },
        )
        .unwrap();
}

#[test]
fn wallet_payment_funds_and_records_the_wallet_as_payer() {
    let (stub, cell) = start_stub();
    let (mut provider, intent) = fixture_provider(&stub);
    store(
        &cell,
        common::fixture_result(
            include_str!("fixtures/synthetic_wallet_pay.json"),
            Some(intent.intent_id.as_str()),
        ),
    );
    fixture_hint(
        &mut provider,
        &intent,
        "agent.fixture.testnet",
        "SynthWalletPay",
    );
    let observation = provider.observe(&intent.intent_id).unwrap();
    assert!(matches!(
        reconcile(&intent, &observation),
        ReconcileOutcome::Fund { .. }
    ));
    let evidence = provider.evidence(&intent.intent_id).expect("evidence");
    assert_eq!(evidence.payer_id, "wallet.fixture.testnet");
    assert_eq!(evidence.predecessor_id, "wallet.fixture.testnet");
    assert_eq!(evidence.executor_id, FIXTURE_ASSET);
    assert!(provider.orphans(&intent.intent_id).is_empty());
}

#[test]
fn a_second_transaction_for_a_funded_intent_becomes_an_orphan() {
    let (stub, cell) = start_stub();
    let (mut provider, intent) = fixture_provider(&stub);
    let memo = intent.intent_id.as_str().to_string();
    store(
        &cell,
        common::fixture_result(
            include_str!("fixtures/synthetic_wallet_pay.json"),
            Some(&memo),
        ),
    );
    fixture_hint(
        &mut provider,
        &intent,
        "agent.fixture.testnet",
        "SynthWalletPay",
    );
    let funded = provider.observe(&intent.intent_id).unwrap();

    // A different transaction paying the same memo arrives later.
    let second = common::fixture_result(
        include_str!("fixtures/synthetic_batch_storage_and_transfer.json"),
        Some(&memo),
    );
    let outcome = aps_near::rpc::parse_tx_outcome("SynthBatchStorageAndTransfer", &second)
        .expect("fixture parses");
    let check = aps_near::check_outcome(provider.intent(&intent.intent_id).unwrap(), &outcome);
    assert!(matches!(
        check.verification,
        aps_near::Verification::Confirmed { .. }
    ));
    let observation = provider.apply_check(&intent.intent_id, check).unwrap();
    // The funding observation is unchanged; the second payment is an orphan.
    assert_eq!(observation, funded);
    let orphans = provider.orphans(&intent.intent_id);
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].tx_hash, "SynthBatchStorageAndTransfer");
    assert_eq!(orphans[0].amount_atomic, "1000000");
    // Applying the same check again does not duplicate the orphan.
    let again = aps_near::check_outcome(provider.intent(&intent.intent_id).unwrap(), &outcome);
    provider.apply_check(&intent.intent_id, again).unwrap();
    assert_eq!(provider.orphans(&intent.intent_id).len(), 1);
}

#[test]
fn a_wrong_amount_is_recorded_as_an_orphan_and_never_funds() {
    let (stub, cell) = start_stub();
    let (mut provider, intent) = fixture_provider(&stub);
    store(
        &cell,
        common::fixture_result(
            include_str!("fixtures/synthetic_wrong_amount.json"),
            Some(intent.intent_id.as_str()),
        ),
    );
    fixture_hint(
        &mut provider,
        &intent,
        "payer.fixture.testnet",
        "SynthWrongAmount",
    );
    let observation = provider.observe(&intent.intent_id).unwrap();
    assert!(matches!(
        reconcile(&intent, &observation),
        ReconcileOutcome::Mismatch { .. }
    ));
    let orphans = provider.orphans(&intent.intent_id);
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].amount_atomic, "2000000");
    // The wrong-amount transfer is not the intent's evidence and does not
    // close it: the exact payment still funds, and the orphan stays listed.
    assert!(provider.evidence(&intent.intent_id).is_none());
    let memo = intent.intent_id.as_str().to_string();
    store(
        &cell,
        common::fixture_result(
            include_str!("fixtures/synthetic_wallet_pay.json"),
            Some(&memo),
        ),
    );
    fixture_hint(
        &mut provider,
        &intent,
        "agent.fixture.testnet",
        "SynthWalletPay",
    );
    let funded = provider.observe(&intent.intent_id).unwrap();
    assert!(matches!(
        reconcile(&intent, &funded),
        ReconcileOutcome::Fund { .. }
    ));
    assert_eq!(
        provider
            .evidence(&intent.intent_id)
            .map(|e| e.tx_hash.as_str()),
        Some("SynthWalletPay")
    );
    assert_eq!(provider.orphans(&intent.intent_id).len(), 1);
}
