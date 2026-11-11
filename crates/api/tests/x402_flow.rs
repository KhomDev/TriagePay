//! x402 v2 transport and facilitator tests (offline: the NEAR provider
//! talks to a loopback stub JSON-RPC server, and the scan runs the real
//! Python scanner subprocess like the other scan tests).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use aps_api::payment::Provider;
use aps_api::x402::{decode_header, encode_header};
use aps_api::{router, AppState};
use aps_near::{NearConfig, NearProvider};
use base64::Engine as _;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::util::ServiceExt;

const GATEWAY: &str = "gateway.x402.testnet";
const ASSET: &str = "usdc.x402.testnet";
const PAYER: &str = "payer.x402.testnet";
const NETWORK: &str = "near:testnet";
const PRICE: &str = "1000000";

type Cell = Arc<Mutex<Option<Value>>>;

/// A loopback JSON-RPC stub: tx_status / EXPERIMENTAL_tx_status answer
/// with the stored outcome, or a not-found error while the cell is empty.
fn stub_server() -> (String, Cell) {
    let cell: Cell = Arc::new(Mutex::new(None));
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
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buffer.extend_from_slice(&chunk[..n]);
                            if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                                let header = String::from_utf8_lossy(&buffer[..end]).to_lowercase();
                                let length = header
                                    .lines()
                                    .find_map(|l| l.strip_prefix("content-length:"))
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                                    .unwrap_or(0);
                                if buffer.len() >= end + 4 + length {
                                    break;
                                }
                            }
                        }
                    }
                }
                let text = String::from_utf8_lossy(&buffer).to_string();
                let body = text.split("\r\n\r\n").nth(1).unwrap_or_default();
                let Ok(request) = serde_json::from_str::<Value>(body) else {
                    return;
                };
                let response = match request.get("method").and_then(Value::as_str) {
                    Some("tx_status") | Some("EXPERIMENTAL_tx_status") => {
                        match handler_cell.lock().map(|g| g.clone()).unwrap_or(None) {
                            Some(outcome) => json!({"jsonrpc": "2.0", "id": 1, "result": outcome}),
                            None => json!({"jsonrpc": "2.0", "id": 1, "error": {
                                "code": -32000, "message": "Server error",
                                "data": "Timeout",
                                "cause": {"name": "TIMEOUT_ERROR", "info": {}}
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
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), cell)
}

fn near_state(url: &str) -> AppState {
    let config = NearConfig {
        rpc_url: url.to_string(),
        network: "testnet".to_string(),
        gateway_account_id: GATEWAY.to_string(),
        asset_contract_id: ASSET.to_string(),
        timeout_secs: 5,
    };
    AppState::with_provider(
        "dev-agent",
        Provider::Near(NearProvider::new(config).expect("provider")),
    )
}

/// A finalized direct ft_transfer (EXPERIMENTAL_tx_status shape).
fn transfer(tx_hash: &str, receiver: &str, memo: &str, amount: &str) -> Value {
    let args = json!({"receiver_id": receiver, "amount": amount, "memo": memo});
    let action = json!({ "FunctionCall": {
        "method_name": "ft_transfer",
        "args": base64::engine::general_purpose::STANDARD.encode(args.to_string()),
        "gas": 30000000000000u64,
        "deposit": "1"
    } });
    let event = json!({
        "standard": "nep141", "version": "1.0.0", "event": "ft_transfer",
        "data": [{ "old_owner_id": PAYER, "new_owner_id": receiver, "amount": amount, "memo": memo }]
    });
    json!({
        "final_execution_status": "FINAL",
        "status": { "SuccessValue": "" },
        "transaction": { "signer_id": PAYER, "receiver_id": ASSET, "actions": [action.clone()], "hash": tx_hash },
        "transaction_outcome": { "id": tx_hash, "outcome": {
            "executor_id": PAYER, "receipt_ids": ["r1"], "status": { "SuccessReceiptId": "r1" }, "logs": [] } },
        "receipts_outcome": [{ "id": "r1", "outcome": {
            "executor_id": ASSET, "status": { "SuccessValue": "" },
            "logs": [format!("EVENT_JSON:{event}")], "receipt_ids": [] } }],
        "receipts": [{ "receipt_id": "r1", "predecessor_id": PAYER, "receiver_id": ASSET,
            "receipt": { "Action": { "actions": [action], "signer_id": PAYER } } }]
    })
}

fn store(cell: &Cell, outcome: Value) {
    *cell.lock().expect("cell") = Some(outcome);
}

fn envelope() -> Value {
    json!({
        "title": "Rounding drain in vault share accounting",
        "claimed_impact": "Permanent loss of user funds via inflated share ratio",
        "mechanism": "First depositor inflates the share price before the pool opens",
        "attacker_capability": "Any external address with collateral",
        "preconditions": "Vault freshly deployed with empty reserves",
        "component_class": "vault",
        "scope_statement": "Vault deposit and mint path is in scope for this review",
        "deployed_state_evidence": "Vault live at 0x1234567890abcdef since block 5100",
        "public_rules_snapshot": "https://archive.example.org/rules/v1",
        "known_issue_declaration": null,
        "reproduction_summary": "Step 1: mint one share. Step 2: donate collateral. Step 3: redeem.",
        "evidence_refs": ["https://example.test/issue-family-1"],
        "code_excerpt": "function mint() external returns (uint256) { ... }",
    })
}

struct Reply {
    status: u16,
    headers: axum::http::HeaderMap,
    body: Value,
}

impl Reply {
    fn header_json(&self, name: &str) -> Value {
        let raw = self
            .headers
            .get(name)
            .unwrap_or_else(|| panic!("missing header {name}"))
            .to_str()
            .expect("ascii");
        decode_header(raw).expect("base64 JSON header")
    }
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    signature: Option<&str>,
    body: Option<Value>,
) -> Reply {
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(sig) = signature {
        builder = builder.header("PAYMENT-SIGNATURE", sig);
    }
    let request = builder
        .body(match body {
            Some(value) => axum::body::Body::from(value.to_string()),
            None => axum::body::Body::empty(),
        })
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("handled");
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Reply {
        status,
        headers,
        body,
    }
}

async fn require(app: &axum::Router) -> Value {
    let reply = send(app, "POST", "/v2/scan", None, Some(envelope())).await;
    assert_eq!(reply.status, 402, "{}", reply.body);
    reply.header_json("payment-required")
}

fn signature(accepted: &Value, tx_hash: &str) -> String {
    let memo = accepted["extra"]["memo"].as_str().expect("memo");
    encode_header(&json!({
        "x402Version": 2,
        "resource": { "url": "/v2/scan" },
        "accepted": accepted,
        "payload": {
            "network": accepted["network"],
            "intent_id": memo,
            "tx_hash": tx_hash,
            "signer_id": PAYER,
        }
    }))
}

async fn balances(app: &axum::Router) -> Value {
    send(app, "GET", "/accounts/x402:dev-agent", None, None)
        .await
        .body
}

#[tokio::test]
async fn payment_required_matches_the_v2_shape() {
    let (url, _cell) = stub_server();
    let app = router(near_state(&url));
    let required = require(&app).await;
    // PaymentRequired (core spec 5.1): x402Version, error, resource, accepts.
    assert_eq!(required["x402Version"], 2);
    assert!(required["error"].is_string());
    assert_eq!(required["resource"]["url"], "/v2/scan");
    assert_eq!(required["resource"]["mimeType"], "application/json");
    let accepts = required["accepts"].as_array().expect("accepts");
    assert_eq!(accepts.len(), 1);
    let req = &accepts[0];
    // PaymentRequirements: the same keys as the transport's example.
    for key in [
        "scheme",
        "network",
        "amount",
        "asset",
        "payTo",
        "maxTimeoutSeconds",
        "extra",
    ] {
        assert!(req.get(key).is_some(), "missing {key}");
    }
    assert_eq!(req["scheme"], "exact");
    assert_eq!(req["network"], NETWORK);
    assert_eq!(req["amount"], PRICE);
    assert!(req["amount"].is_string(), "amounts are atomic strings");
    assert_eq!(req["asset"], ASSET);
    assert_eq!(req["payTo"], GATEWAY);
    assert!(req["maxTimeoutSeconds"].is_u64());
    assert_eq!(req["extra"]["paymentFlow"], "upfront");
    assert_eq!(req["extra"]["assetTransferMethod"], "near-tx-proof");
    let memo = req["extra"]["memo"].as_str().expect("memo");
    assert!(memo.starts_with("aps2_"));
    assert_eq!(req["extra"]["intentId"], memo);
}

#[tokio::test]
async fn pay_then_retry_settles_once_and_replays() {
    let (url, cell) = stub_server();
    let app = router(near_state(&url));
    let required = require(&app).await;
    let accepted = required["accepts"][0].clone();
    let memo = accepted["extra"]["memo"].as_str().unwrap().to_string();
    let sig = signature(&accepted, "tx-paid");

    // Not final yet: 402 settlement_pending with the transaction hash.
    let pending = send(&app, "POST", "/v2/scan", Some(&sig), Some(envelope())).await;
    assert_eq!(pending.status, 402, "{}", pending.body);
    let header = pending.header_json("payment-response");
    assert_eq!(header["success"], false);
    assert_eq!(header["errorReason"], "settlement_pending");
    assert_eq!(header["transaction"], "tx-paid");
    assert!(pending.headers.get("retry-after").is_some());

    // The transfer is final: the same proof now settles.
    store(&cell, transfer("tx-paid", GATEWAY, &memo, PRICE));
    let paid = send(&app, "POST", "/v2/scan", Some(&sig), Some(envelope())).await;
    assert_eq!(paid.status, 200, "{}", paid.body);
    let header = paid.header_json("payment-response");
    assert_eq!(header["success"], true);
    assert_eq!(header["transaction"], "tx-paid");
    assert_eq!(header["network"], NETWORK);
    assert_eq!(header["payer"], PAYER);
    assert_eq!(header["amount"], PRICE);
    assert_eq!(
        header["extensions"]["aps-ledger"]["intentId"],
        memo.as_str()
    );
    assert_eq!(paid.body["report"]["decision"], "supported");
    assert_eq!(paid.body["settlement"]["charged"]["atomic"], PRICE);

    // Replay: same proof, same request: identical response, no new charge.
    let replay = send(&app, "POST", "/v2/scan", Some(&sig), Some(envelope())).await;
    assert_eq!(replay.status, 200);
    assert_eq!(replay.body, paid.body);
    assert_eq!(replay.header_json("payment-response"), header);
    let b = balances(&app).await;
    assert_eq!(b["deposited_total"]["atomic"], PRICE);
    assert_eq!(b["settled_total"]["atomic"], PRICE);
    assert_eq!(b["available"]["atomic"], "0");

    // The same proof for a different request is refused.
    let mut other = envelope();
    other["title"] = json!("A different finding");
    let reused = send(&app, "POST", "/v2/scan", Some(&sig), Some(other)).await;
    assert_eq!(reused.status, 402);
    assert_eq!(
        reused.header_json("payment-response")["errorReason"],
        "proof_already_consumed"
    );
    assert_eq!(balances(&app).await["settled_total"]["atomic"], PRICE);
}

#[tokio::test]
async fn a_proof_never_funds_a_second_requirement() {
    let (url, cell) = stub_server();
    let app = router(near_state(&url));
    let first = require(&app).await["accepts"][0].clone();
    let memo = first["extra"]["memo"].as_str().unwrap().to_string();
    store(&cell, transfer("tx-one", GATEWAY, &memo, PRICE));
    let paid = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&first, "tx-one")),
        Some(envelope()),
    )
    .await;
    assert_eq!(paid.status, 200, "{}", paid.body);

    // A second requirement, presented with the first transaction.
    let second = require(&app).await["accepts"][0].clone();
    let reused = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&second, "tx-one")),
        Some(envelope()),
    )
    .await;
    assert_eq!(reused.status, 402);
    assert_eq!(
        reused.header_json("payment-response")["errorReason"],
        "proof_already_consumed"
    );
    let b = balances(&app).await;
    assert_eq!(b["deposited_total"]["atomic"], PRICE);
    assert_eq!(b["settled_total"]["atomic"], PRICE);
}

#[tokio::test]
async fn wrong_amount_wrong_network_wrong_pay_to_never_fund() {
    let (url, cell) = stub_server();
    let app = router(near_state(&url));

    // Wrong amount on chain.
    let accepted = require(&app).await["accepts"][0].clone();
    let memo = accepted["extra"]["memo"].as_str().unwrap().to_string();
    store(&cell, transfer("tx-short", GATEWAY, &memo, "999999"));
    let reply = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&accepted, "tx-short")),
        Some(envelope()),
    )
    .await;
    assert_eq!(reply.status, 402);
    assert_eq!(
        reply.header_json("payment-response")["errorReason"],
        "amount_mismatch"
    );

    // Wrong network in the payload.
    let accepted = require(&app).await["accepts"][0].clone();
    let mut wrong_network = accepted.clone();
    wrong_network["network"] = json!("near:mainnet");
    let reply = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&wrong_network, "tx-x")),
        Some(envelope()),
    )
    .await;
    assert_eq!(reply.status, 402);
    assert_eq!(
        reply.header_json("payment-response")["errorReason"],
        "invalid_network"
    );

    // Wrong payTo in the accepted requirements.
    let mut wrong_pay_to = accepted.clone();
    wrong_pay_to["payTo"] = json!("attacker.testnet");
    let reply = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&wrong_pay_to, "tx-x")),
        Some(envelope()),
    )
    .await;
    assert_eq!(reply.status, 402);
    assert_eq!(
        reply.header_json("payment-response")["errorReason"],
        "invalid_payment_requirements"
    );

    // A transfer to another recipient with the right memo.
    let memo = accepted["extra"]["memo"].as_str().unwrap().to_string();
    store(
        &cell,
        transfer("tx-elsewhere", "attacker.testnet", &memo, PRICE),
    );
    let reply = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&accepted, "tx-elsewhere")),
        Some(envelope()),
    )
    .await;
    assert_eq!(reply.status, 402);
    assert_eq!(
        reply.header_json("payment-response")["errorReason"],
        "invalid_payload"
    );

    assert_eq!(balances(&app).await["error"]["code"], "unknown_account");
}

#[tokio::test]
async fn an_expired_requirement_never_funds() {
    let (url, cell) = stub_server();
    let state = near_state(&url).with_x402_terms(aps_core::Amount::from_atomic(1_000_000), 1);
    let app = router(state);
    let accepted = require(&app).await["accepts"][0].clone();
    assert_eq!(accepted["maxTimeoutSeconds"], 1);
    let memo = accepted["extra"]["memo"].as_str().unwrap().to_string();
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    store(&cell, transfer("tx-late", GATEWAY, &memo, PRICE));
    let reply = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&accepted, "tx-late")),
        Some(envelope()),
    )
    .await;
    assert_eq!(reply.status, 402);
    assert_eq!(
        reply.header_json("payment-response")["errorReason"],
        "requirement_expired"
    );
    assert_eq!(balances(&app).await["error"]["code"], "unknown_account");

    // The transfer landed, so it is on record for an operator refund.
    let intent = send(&app, "GET", &format!("/payment/intents/{memo}"), None, None).await;
    assert_eq!(intent.status, 200, "{}", intent.body);
    assert_eq!(intent.body["status"], "expired");
    let orphans = intent.body["orphans"].as_array().expect("orphans");
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0]["tx_hash"], "tx-late");
    assert_eq!(orphans[0]["payer_id"], PAYER);
    assert_eq!(orphans[0]["amount_atomic"], PRICE);
    assert!(orphans[0]["reason"]
        .as_str()
        .is_some_and(|reason| reason.contains("expired")));
}

#[tokio::test]
async fn expired_requirements_nobody_paid_are_forgotten() {
    let (url, _cell) = stub_server();
    let state = near_state(&url)
        .with_x402_terms(aps_core::Amount::from_atomic(1_000_000), 1)
        .with_x402_prune_interval(0);
    let app = router(state);
    let first = require(&app).await["accepts"][0].clone();
    let memo = first["extra"]["memo"].as_str().unwrap().to_string();
    let known = send(&app, "GET", &format!("/payment/intents/{memo}"), None, None).await;
    assert_eq!(known.status, 200);

    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    // Issuing the next requirement sweeps the expired, unpaid one.
    let _second = require(&app).await;
    let gone = send(&app, "GET", &format!("/payment/intents/{memo}"), None, None).await;
    assert_eq!(gone.status, 404, "{}", gone.body);
    let reply = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&first, "tx-any")),
        Some(envelope()),
    )
    .await;
    assert_eq!(reply.status, 402);
    assert_eq!(
        reply.header_json("payment-response")["errorReason"],
        "invalid_payload"
    );
}

#[tokio::test]
async fn malformed_headers_are_400() {
    let (url, _cell) = stub_server();
    let app = router(near_state(&url));
    let reply = send(
        &app,
        "POST",
        "/v2/scan",
        Some("%%%not-base64%%%"),
        Some(envelope()),
    )
    .await;
    assert_eq!(reply.status, 400);
    assert_eq!(
        reply.header_json("payment-response")["errorReason"],
        "invalid_payload"
    );
    let v1 = encode_header(&json!({"x402Version": 1, "accepted": {}, "payload": {}}));
    let reply = send(&app, "POST", "/v2/scan", Some(&v1), Some(envelope())).await;
    assert_eq!(reply.status, 400);
    assert_eq!(
        reply.header_json("payment-response")["errorReason"],
        "invalid_x402_version"
    );
    let reply = send(&app, "POST", "/v2/scan", None, Some(json!([1, 2]))).await;
    assert_eq!(reply.status, 400);
    assert_eq!(reply.body["error"]["code"], "validation_error");
}

#[tokio::test]
async fn a_concurrent_retry_storm_charges_once() {
    let (url, cell) = stub_server();
    let app = router(near_state(&url));
    let accepted = require(&app).await["accepts"][0].clone();
    let memo = accepted["extra"]["memo"].as_str().unwrap().to_string();
    store(&cell, transfer("tx-storm", GATEWAY, &memo, PRICE));
    let sig = signature(&accepted, "tx-storm");
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let app = app.clone();
        let sig = sig.clone();
        tasks.push(tokio::spawn(async move {
            send(&app, "POST", "/v2/scan", Some(&sig), Some(envelope())).await
        }));
    }
    let mut bodies = Vec::new();
    for task in tasks {
        let reply = task.await.expect("task");
        assert_eq!(reply.status, 200, "{}", reply.body);
        bodies.push(reply.body);
    }
    assert!(
        bodies.windows(2).all(|w| w[0] == w[1]),
        "every retry sees one settlement"
    );
    let b = balances(&app).await;
    assert_eq!(b["deposited_total"]["atomic"], PRICE);
    assert_eq!(b["settled_total"]["atomic"], PRICE);
}

#[tokio::test]
async fn the_mock_provider_speaks_the_same_protocol_on_a_dev_network() {
    let app = router(AppState::new());
    let accepted = require(&app).await["accepts"][0].clone();
    assert_eq!(accepted["network"], "aps:mock");
    let reply = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&accepted, "mock-tx")),
        Some(envelope()),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.header_json("payment-response")["success"], true);
}

// ----- facilitator -----------------------------------------------------------

fn third_party_requirements(memo: &str) -> Value {
    json!({
        "scheme": "exact",
        "network": NETWORK,
        "amount": "250000",
        "asset": ASSET,
        "payTo": "merchant.x402.testnet",
        "maxTimeoutSeconds": 600,
        "extra": { "assetTransferMethod": "near-tx-proof", "paymentFlow": "upfront", "memo": memo }
    })
}

fn facilitator_body(requirements: &Value, tx: &str) -> Value {
    json!({
        "x402Version": 2,
        "paymentRequirements": requirements,
        "paymentPayload": {
            "x402Version": 2,
            "accepted": requirements,
            "payload": {
                "network": NETWORK,
                "intent_id": requirements["extra"]["memo"],
                "tx_hash": tx,
                "signer_id": PAYER,
            }
        }
    })
}

#[tokio::test]
async fn facilitator_supported_verify_and_settle() {
    let (url, cell) = stub_server();
    let app = router(near_state(&url));
    let supported = send(&app, "GET", "/x402/supported", None, None).await;
    assert_eq!(supported.status, 200);
    assert_eq!(supported.body["kinds"][0]["x402Version"], 2);
    assert_eq!(supported.body["kinds"][0]["scheme"], "exact");
    assert_eq!(supported.body["kinds"][0]["network"], NETWORK);
    assert_eq!(
        supported.body["kinds"][0]["extra"]["assetTransferMethod"],
        "near-tx-proof"
    );
    assert!(supported.body["extensions"].is_array());
    assert!(supported.body["signers"].is_object());

    let requirements = third_party_requirements("merchant-order-77");
    let body = facilitator_body(&requirements, "tx-merchant");

    // Not final yet.
    let reply = send(&app, "POST", "/x402/verify", None, Some(body.clone())).await;
    assert_eq!(
        reply.body,
        json!({"isValid": false, "invalidReason": "settlement_pending"})
    );
    let reply = send(&app, "POST", "/x402/settle", None, Some(body.clone())).await;
    assert_eq!(reply.body["success"], false);
    assert_eq!(reply.body["errorReason"], "settlement_pending");
    assert_eq!(reply.body["transaction"], "tx-merchant");

    // Final: verify is read-only and repeatable; settle succeeds once.
    store(
        &cell,
        transfer(
            "tx-merchant",
            "merchant.x402.testnet",
            "merchant-order-77",
            "250000",
        ),
    );
    for _ in 0..2 {
        let reply = send(&app, "POST", "/x402/verify", None, Some(body.clone())).await;
        assert_eq!(reply.body, json!({"isValid": true, "payer": PAYER}));
    }
    let settled = send(&app, "POST", "/x402/settle", None, Some(body.clone())).await;
    assert_eq!(settled.body["success"], true);
    assert_eq!(settled.body["transaction"], "tx-merchant");
    assert_eq!(settled.body["network"], NETWORK);
    assert_eq!(settled.body["payer"], PAYER);
    assert_eq!(settled.body["amount"], "250000");
    let again = send(&app, "POST", "/x402/settle", None, Some(body.clone())).await;
    assert_eq!(again.body["success"], false);
    assert_eq!(again.body["errorReason"], "proof_already_consumed");
    let after = send(&app, "POST", "/x402/verify", None, Some(body)).await;
    assert_eq!(after.body["invalidReason"], "proof_already_consumed");
}

#[tokio::test]
async fn facilitator_rejects_mismatches_and_this_servers_own_intents() {
    let (url, cell) = stub_server();
    let app = router(near_state(&url));
    store(
        &cell,
        transfer("tx-m", "merchant.x402.testnet", "order-1", "250000"),
    );
    // Amount on chain differs from the requirements.
    let mut requirements = third_party_requirements("order-1");
    requirements["amount"] = json!("300000");
    let reply = send(
        &app,
        "POST",
        "/x402/verify",
        None,
        Some(facilitator_body(&requirements, "tx-m")),
    )
    .await;
    assert_eq!(reply.body["invalidReason"], "amount_mismatch");
    // Memo differs: the event does not carry it.
    let requirements = third_party_requirements("order-2");
    let reply = send(
        &app,
        "POST",
        "/x402/verify",
        None,
        Some(facilitator_body(&requirements, "tx-m")),
    )
    .await;
    assert_eq!(reply.body["invalidReason"], "invalid_payload");
    // accepted differs from the requirements under verification.
    let requirements = third_party_requirements("order-1");
    let mut body = facilitator_body(&requirements, "tx-m");
    body["paymentPayload"]["accepted"]["amount"] = json!("1");
    let reply = send(&app, "POST", "/x402/verify", None, Some(body)).await;
    assert_eq!(reply.body["invalidReason"], "invalid_payment_requirements");
    // An intent issued by this server's own /v2/scan cannot be settled
    // here by a third party (front-running the payer's own presentation).
    let own = require(&app).await["accepts"][0].clone();
    let memo = own["extra"]["memo"].as_str().unwrap().to_string();
    store(&cell, transfer("tx-own", GATEWAY, &memo, PRICE));
    let reply = send(
        &app,
        "POST",
        "/x402/settle",
        None,
        Some(facilitator_body(&own, "tx-own")),
    )
    .await;
    assert_eq!(reply.body["success"], false);
    assert_eq!(reply.body["errorReason"], "invalid_payment_requirements");
    // Wrong version and malformed bodies.
    let reply = send(
        &app,
        "POST",
        "/x402/verify",
        None,
        Some(json!({"x402Version": 1})),
    )
    .await;
    assert_eq!(reply.body["invalidReason"], "invalid_x402_version");
    let reply = send(&app, "POST", "/x402/verify", None, Some(json!([]))).await;
    assert_eq!(reply.status, 400);
}

// ----- a hint anyone can submit never decides a requirement's fate -----------

/// A finalized transaction that failed and has nothing to do with the
/// settlement asset, the gateway or any memo: a plain NEAR transfer by
/// another account to an account that does not exist.
fn unrelated_failed_tx(tx_hash: &str) -> Value {
    let failure = json!({ "Failure": { "ActionError": { "index": 0,
        "kind": { "AccountDoesNotExist": { "account_id": "nobody.testnet" } } } } });
    let action = json!({ "Transfer": { "deposit": "1" } });
    json!({
        "final_execution_status": "FINAL",
        "status": failure.clone(),
        "transaction": { "signer_id": "mallory.testnet", "receiver_id": "nobody.testnet",
            "actions": [action.clone()], "hash": tx_hash },
        "transaction_outcome": { "id": tx_hash, "outcome": {
            "executor_id": "mallory.testnet", "receipt_ids": ["f1"],
            "status": { "SuccessReceiptId": "f1" }, "logs": [] } },
        "receipts_outcome": [{ "id": "f1", "outcome": {
            "executor_id": "nobody.testnet", "status": failure,
            "logs": [], "receipt_ids": [] } }],
        "receipts": [{ "receipt_id": "f1", "predecessor_id": "mallory.testnet",
            "receiver_id": "nobody.testnet",
            "receipt": { "Action": { "actions": [action], "signer_id": "mallory.testnet" } } }]
    })
}

/// A third party hints a transaction at someone else's pending
/// requirement through the observation route.
async fn third_party_hint(app: &axum::Router, memo: &str, tx_hash: &str) -> Reply {
    send(
        app,
        "POST",
        &format!("/payment/intents/{memo}/observations"),
        None,
        Some(json!({ "signer_id": "mallory.testnet", "tx_hash": tx_hash })),
    )
    .await
}

#[tokio::test]
async fn an_unrelated_failed_transaction_does_not_block_the_real_payment() {
    let (url, cell) = stub_server();
    let app = router(near_state(&url));
    let accepted = require(&app).await["accepts"][0].clone();
    let memo = accepted["extra"]["memo"].as_str().unwrap().to_string();

    // The failed transaction is reported for that hint, by both routes...
    store(&cell, unrelated_failed_tx("tx-unrelated-failed"));
    let hinted = third_party_hint(&app, &memo, "tx-unrelated-failed").await;
    assert_eq!(hinted.status, 200, "{}", hinted.body);
    assert_eq!(hinted.body["reconciled"], "rejected");
    assert_eq!(hinted.body["funded"], false);
    let presented = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&accepted, "tx-unrelated-failed")),
        Some(envelope()),
    )
    .await;
    assert_eq!(presented.status, 402);
    assert_eq!(
        presented.header_json("payment-response")["errorReason"],
        "invalid_transaction_state"
    );

    // ...and the requirement is still open: the real payment settles it.
    store(&cell, transfer("tx-good", GATEWAY, &memo, PRICE));
    let paid = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&accepted, "tx-good")),
        Some(envelope()),
    )
    .await;
    assert_eq!(paid.status, 200, "{}", paid.body);
    assert_eq!(
        paid.header_json("payment-response")["transaction"],
        "tx-good"
    );
    let b = balances(&app).await;
    assert_eq!(b["deposited_total"]["atomic"], PRICE);
    assert_eq!(b["settled_total"]["atomic"], PRICE);
}

#[tokio::test]
async fn a_dust_transfer_with_the_memo_does_not_block_the_exact_payment() {
    let (url, cell) = stub_server();
    let app = router(near_state(&url));
    let accepted = require(&app).await["accepts"][0].clone();
    let memo = accepted["extra"]["memo"].as_str().unwrap().to_string();

    // One atomic unit to the gateway with the memo: a mismatch, no credit.
    store(&cell, transfer("tx-dust", GATEWAY, &memo, "1"));
    let hinted = third_party_hint(&app, &memo, "tx-dust").await;
    assert_eq!(hinted.status, 200, "{}", hinted.body);
    assert_eq!(hinted.body["reconciled"], "mismatch");
    assert_eq!(hinted.body["funded"], false);
    assert_eq!(balances(&app).await["error"]["code"], "unknown_account");

    // The exact payment still funds and settles the requirement, once.
    store(&cell, transfer("tx-good", GATEWAY, &memo, PRICE));
    let paid = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&accepted, "tx-good")),
        Some(envelope()),
    )
    .await;
    assert_eq!(paid.status, 200, "{}", paid.body);
    let b = balances(&app).await;
    assert_eq!(b["deposited_total"]["atomic"], PRICE);
    assert_eq!(b["settled_total"]["atomic"], PRICE);

    // Presenting the dust transfer again changes nothing.
    store(&cell, transfer("tx-dust", GATEWAY, &memo, "1"));
    let again = send(
        &app,
        "POST",
        "/v2/scan",
        Some(&signature(&accepted, "tx-dust")),
        Some(envelope()),
    )
    .await;
    assert_eq!(again.status, 402);
    assert_eq!(
        again.header_json("payment-response")["errorReason"],
        "proof_already_consumed"
    );
    assert_eq!(balances(&app).await["deposited_total"]["atomic"], PRICE);
}

#[tokio::test]
async fn requirement_memos_carry_an_unguessable_nonce() {
    let (url, _cell) = stub_server();
    let app = router(near_state(&url));
    let mut nonces = Vec::new();
    for _ in 0..16 {
        let accepted = require(&app).await["accepts"][0].clone();
        let memo = accepted["extra"]["memo"].as_str().unwrap().to_string();
        assert_eq!(accepted["extra"]["intentId"], memo.as_str());
        let (_, nonce) = memo.rsplit_once('_').expect("memo segments");
        assert_eq!(nonce.len(), 32, "a 128-bit nonce in hex: {memo}");
        nonces.push(u128::from_str_radix(nonce, 16).expect("hex nonce"));
    }
    for pair in nonces.windows(2) {
        assert_ne!(pair[0], pair[1]);
        assert_ne!(pair[1], pair[0].wrapping_add(1), "memos must not count up");
    }
}

#[tokio::test]
async fn dev_only_routes_refuse_under_the_near_provider() {
    let (url, _cell) = stub_server();
    let app = router(near_state(&url));

    // Direct credit: no payment, so no credit and no account.
    let reply = send(
        &app,
        "POST",
        "/accounts/freeloader/deposits",
        None,
        Some(json!({ "amount_atomic": "1000000000", "idempotency_key": "free-1" })),
    )
    .await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.body["error"]["code"], "dev_only_route");
    let account = send(&app, "GET", "/accounts/freeloader", None, None).await;
    assert_eq!(account.status, 404);

    // Settlement against a caller-supplied hash: refused before the job
    // lookup, whatever the job id.
    let reply = send(
        &app,
        "POST",
        "/jobs/job_0000000000000001/settlements",
        None,
        Some(json!({ "report_hash_hex": "ab".repeat(32) })),
    )
    .await;
    assert_eq!(reply.status, 403, "{}", reply.body);
    assert_eq!(reply.body["error"]["code"], "dev_only_route");
}
