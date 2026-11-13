//! Route-level tests for the NEAR payment provider: the full funding
//! path over the HTTP API against a deterministic stub JSON-RPC server
//! (offline, loopback only). Mirrors the mock provider's payment_flow
//! suite: a memo-bound, on-chain-verified deposit funds credit exactly
//! once, and hints/amount mismatches never fund.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use aps_api::payment::Provider;
use aps_api::{router, AppState};
use aps_near::{NearConfig, NearProvider};
use base64::Engine as _;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::util::ServiceExt;

const GATEWAY: &str = "gateway.near";
const ASSET: &str = "asset.near";
const SIGNER: &str = "payer.near";

fn stub_server() -> (String, Arc<Mutex<Option<Value>>>) {
    let cell: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let handler_cell = Arc::clone(&cell);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let handler_cell = Arc::clone(&handler_cell);
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buffer.extend_from_slice(&chunk[..n]);
                            if let Some(header_end) =
                                buffer.windows(4).position(|w| w == b"\r\n\r\n")
                            {
                                let header =
                                    String::from_utf8_lossy(&buffer[..header_end]).to_string();
                                let content_length = header
                                    .lines()
                                    .find_map(|line| {
                                        if line.to_ascii_lowercase().starts_with("content-length:")
                                        {
                                            line.split(':')
                                                .nth(1)
                                                .and_then(|v| v.trim().parse::<usize>().ok())
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                if buffer.len() >= header_end + 4 + content_length {
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                let body = String::from_utf8_lossy(&buffer).to_string();
                let json_body = body.split("\r\n\r\n").nth(1).unwrap_or_default();
                let request: Value = match serde_json::from_str(json_body) {
                    Ok(value) => value,
                    Err(_) => return,
                };
                let response = match request.get("method").and_then(Value::as_str) {
                    Some("tx_status") | Some("EXPERIMENTAL_tx_status") => {
                        let stored = handler_cell.lock().map(|g| g.clone()).unwrap_or(None);
                        match stored {
                            Some(outcome) => json!({"jsonrpc": "2.0", "id": 1, "result": outcome}),
                            None => json!({"jsonrpc": "2.0", "id": 1, "error": {
                                "code": -32000, "message": "Server error",
                                "data": "Tx txhash not found"
                            }}),
                        }
                    }
                    _ => json!({"jsonrpc": "2.0", "id": 1, "result": Value::Null}),
                };
                let text = response.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    text.len(),
                    text
                );
                let _ = stream.flush();
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), cell)
}

fn near_app(url: &str) -> axum::Router {
    let config = NearConfig {
        rpc_url: url.to_string(),
        network: "mainnet".to_string(),
        gateway_account_id: GATEWAY.to_string(),
        asset_contract_id: ASSET.to_string(),
        timeout_secs: 5,
    };
    let provider = NearProvider::new(config).expect("provider builds");
    router(AppState::with_provider(
        "dev-agent",
        Provider::Near(provider),
    ))
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("body is JSON")
}

async fn post(app: axum::Router, uri: &str, body: Option<Value>) -> (u16, Value) {
    let request = match body {
        Some(value) => axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(value.to_string()))
            .expect("request builds"),
        None => axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .body(axum::body::Body::empty())
            .expect("request builds"),
    };
    let response = app.oneshot(request).await.expect("request handled");
    let status = response.status().as_u16();
    (status, body_json(response).await)
}

async fn balances_of(app: axum::Router, account: &str) -> Value {
    let request = axum::http::Request::builder()
        .uri(format!("/accounts/{account}"))
        .body(axum::body::Body::empty())
        .expect("request builds");
    let response = app.oneshot(request).await.expect("request handled");
    body_json(response).await
}

/// A finalized direct ft_transfer in the EXPERIMENTAL_tx_status shape:
/// the token receipt carries the nep141 event the verifier binds to.
fn store_tx(cell: &Arc<Mutex<Option<Value>>>, memo: &str, amount: &str) {
    let args = json!({"receiver_id": GATEWAY, "amount": amount, "memo": memo});
    let args_base64 = base64::engine::general_purpose::STANDARD.encode(args.to_string().as_bytes());
    let action = json!({ "FunctionCall": {
        "method_name": "ft_transfer",
        "args": args_base64,
        "gas": 30000000000000u64,
        "deposit": "1"
    } });
    let event = json!({
        "standard": "nep141", "version": "1.0.0", "event": "ft_transfer",
        "data": [{ "old_owner_id": SIGNER, "new_owner_id": GATEWAY, "amount": amount, "memo": memo }]
    });
    let outcome = json!({
        "final_execution_status": "FINAL",
        "status": { "SuccessValue": "" },
        "transaction": {
            "signer_id": SIGNER,
            "receiver_id": ASSET,
            "actions": [ action.clone() ],
            "hash": "txhash"
        },
        "transaction_outcome": {
            "id": "txhash",
            "outcome": { "executor_id": SIGNER, "receipt_ids": ["r1"],
                         "status": { "SuccessReceiptId": "r1" }, "logs": [] }
        },
        "receipts_outcome": [{
            "id": "r1",
            "outcome": { "executor_id": ASSET, "status": { "SuccessValue": "" },
                         "logs": [format!("EVENT_JSON:{event}")], "receipt_ids": [] }
        }],
        "receipts": [{
            "receipt_id": "r1", "predecessor_id": SIGNER, "receiver_id": ASSET,
            "receipt": { "Action": { "actions": [ action ], "signer_id": SIGNER } }
        }]
    });
    *cell.lock().expect("cell lock") = Some(outcome);
}

#[tokio::test]
async fn near_deposit_funds_credit_exactly_once_through_the_routes() {
    let (url, cell) = stub_server();
    let app = near_app(&url);

    // 1. Create the intent: the response carries near deposit instructions.
    let (status, intent) = post(
        app.clone(),
        "/payment/intents",
        Some(json!({
            "account_id": "acct-near",
            "asset_id": ASSET,
            "amount_atomic": "1000000",
            "expires_in_secs": 3600,
        })),
    )
    .await;
    assert_eq!(status, 201, "{intent}");
    assert_eq!(intent["provider"], "near");
    assert_eq!(intent["deposit_address"], GATEWAY);
    let intent_id = intent["intent_id"].as_str().expect("intent id").to_string();
    assert_eq!(
        intent["memo"].as_str().expect("memo is the intent id"),
        intent_id
    );

    // 2. Observing without a hint body is a 400 for the near provider.
    let (status, error) = post(
        app.clone(),
        &format!("/payment/intents/{intent_id}/observations"),
        None,
    )
    .await;
    assert_eq!(status, 400, "{error}");
    assert_eq!(error["error"]["code"], "validation_error");

    // 3. A hint before the transaction is visible stays pending.
    let (status, observation) = post(
        app.clone(),
        &format!("/payment/intents/{intent_id}/observations"),
        Some(json!({"signer_id": SIGNER, "tx_hash": "txhash"})),
    )
    .await;
    assert_eq!(status, 200, "{observation}");
    assert_eq!(observation["reconciled"], "still_pending");
    assert_eq!(observation["funded"], false);

    // 4. The payer's transfer lands on chain (the stub now answers).
    store_tx(&cell, &intent_id, "1000000");
    let (status, observation) = post(
        app.clone(),
        &format!("/payment/intents/{intent_id}/observations"),
        Some(json!({"signer_id": SIGNER, "tx_hash": "txhash"})),
    )
    .await;
    assert_eq!(status, 200, "{observation}");
    assert_eq!(observation["reconciled"], "fund");
    assert_eq!(observation["funded"], true);
    assert_eq!(observation["tx_ref"], "txhash");

    // 5. The account was credited exactly once.
    let balances = balances_of(app.clone(), "acct-near").await;
    assert_eq!(balances["deposited_total"]["atomic"], "1000000");
    assert_eq!(balances["available"]["atomic"], "1000000");

    // 6. Re-observing replays: still funded, still exactly one deposit.
    let (status, replay) = post(
        app.clone(),
        &format!("/payment/intents/{intent_id}/observations"),
        Some(json!({"signer_id": SIGNER, "tx_hash": "txhash"})),
    )
    .await;
    assert_eq!(status, 200, "{replay}");
    assert_eq!(replay["funded"], true);
    let balances = balances_of(app.clone(), "acct-near").await;
    assert_eq!(balances["deposited_total"]["atomic"], "1000000");

    // 7. Refunds are proposals only, one shot.
    let (status, refund) = post(
        app.clone(),
        &format!("/payment/intents/{intent_id}/refunds"),
        Some(json!({"reason": "operator requested"})),
    )
    .await;
    assert_eq!(status, 200, "{refund}");
    assert!(refund["reason"]
        .as_str()
        .expect("reason")
        .contains("PROPOSAL ONLY"));
    let (status, again) = post(
        app.clone(),
        &format!("/payment/intents/{intent_id}/refunds"),
        Some(json!({"reason": "second attempt"})),
    )
    .await;
    assert_eq!(status, 409, "{again}");
}

#[tokio::test]
async fn near_amount_mismatch_surfaces_without_funding() {
    let (url, cell) = stub_server();
    let app = near_app(&url);
    let (status, intent) = post(
        app.clone(),
        "/payment/intents",
        Some(json!({
            "account_id": "acct-mm",
            "asset_id": ASSET,
            "amount_atomic": "1000000",
            "expires_in_secs": 3600,
        })),
    )
    .await;
    assert_eq!(status, 201);
    let intent_id = intent["intent_id"].as_str().expect("intent id").to_string();
    // The payer sent 2 USDC where 1 USDC was asked for.
    store_tx(&cell, &intent_id, "2000000");
    let (status, observation) = post(
        app.clone(),
        &format!("/payment/intents/{intent_id}/observations"),
        Some(json!({"signer_id": SIGNER, "tx_hash": "txhash"})),
    )
    .await;
    assert_eq!(status, 200, "{observation}");
    assert_eq!(observation["reconciled"], "mismatch");
    assert_eq!(observation["funded"], false);
    let balances = balances_of(app, "acct-mm").await;
    assert_eq!(balances["error"]["code"], "unknown_account");
}

#[tokio::test]
async fn near_mock_scenarios_and_foreign_assets_are_rejected() {
    let (url, _cell) = stub_server();
    let app = near_app(&url);
    let (status, error) = post(
        app.clone(),
        "/payment/intents",
        Some(json!({
            "account_id": "acct-sc",
            "asset_id": ASSET,
            "amount_atomic": "1000000",
            "expires_in_secs": 3600,
            "mock_scenario": {"type": "never_confirm"}
        })),
    )
    .await;
    assert_eq!(status, 400, "{error}");
    let (status, error) = post(
        app.clone(),
        "/payment/intents",
        Some(json!({
            "account_id": "acct-sc",
            "asset_id": "other.near",
            "amount_atomic": "1000000",
            "expires_in_secs": 3600,
        })),
    )
    .await;
    assert_eq!(status, 400, "{error}");
}

/// Red-team RT-01: with a provider that settles real payments, the server
/// prices a reservation from its price book. A caller-chosen amount below
/// the price is refused, an unsold SKU is refused, and an omitted amount
/// reserves exactly the book price.
#[tokio::test]
async fn near_reservations_are_priced_by_the_server() {
    let (url, cell) = stub_server();
    let app = near_app(&url);
    let (status, intent) = post(
        app.clone(),
        "/payment/intents",
        Some(json!({
            "account_id": "acct-priced",
            "asset_id": ASSET,
            "amount_atomic": "2000000",
            "expires_in_secs": 3600,
        })),
    )
    .await;
    assert_eq!(status, 201, "{intent}");
    let intent_id = intent["intent_id"].as_str().expect("intent id").to_string();
    store_tx(&cell, &intent_id, "2000000");
    let (status, observation) = post(
        app.clone(),
        &format!("/payment/intents/{intent_id}/observations"),
        Some(json!({"signer_id": SIGNER, "tx_hash": "txhash"})),
    )
    .await;
    assert_eq!(status, 200, "{observation}");
    assert_eq!(observation["funded"], true);

    // One atomic unit for a full scan: refused, nothing reserved.
    let (status, error) = post(
        app.clone(),
        "/reservations",
        Some(json!({
            "account_id": "acct-priced",
            "sku": "scan.v1",
            "amount_atomic": "1",
            "idempotency_key": "cheap",
        })),
    )
    .await;
    assert_eq!(status, 400, "{error}");
    assert_eq!(error["error"]["code"], "validation_error");

    // A SKU the scan route does not run: refused.
    let (status, error) = post(
        app.clone(),
        "/reservations",
        Some(json!({
            "account_id": "acct-priced",
            "sku": "triage.v2",
            "idempotency_key": "other-sku",
        })),
    )
    .await;
    assert_eq!(status, 400, "{error}");

    let balances = balances_of(app.clone(), "acct-priced").await;
    assert_eq!(balances["reserved"]["atomic"], "0");

    // No amount: the book price is reserved.
    let (status, reservation) = post(
        app.clone(),
        "/reservations",
        Some(json!({
            "account_id": "acct-priced",
            "sku": "scan.v1",
            "idempotency_key": "priced",
        })),
    )
    .await;
    assert_eq!(status, 201, "{reservation}");
    assert_eq!(reservation["reserved"]["atomic"], "1000000");
}
