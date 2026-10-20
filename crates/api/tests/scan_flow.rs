//! End-to-end scan flow: pay, run the scanner, settle against the hash.
//!
//! These tests execute the real Python scanner as a subprocess, exactly
//! as production would: no mocks between the API and the engine.

use std::path::PathBuf;
use std::time::Duration;

use aps_api::scanbridge::ScanLauncher;
use aps_api::{router, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn json_request(method: &str, uri: String, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    match body {
        Some(value) => builder
            .body(Body::from(value.to_string()))
            .expect("request body builds"),
        None => builder.body(Body::empty()).expect("request body builds"),
    }
}

fn strong_envelope() -> Value {
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

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is JSON")
}

async fn deposit(app: axum::Router, account: &str, atomic: &str) {
    let response = app
        .oneshot(json_request(
            "POST",
            format!("/accounts/{account}/deposits"),
            Some(json!({
                "amount_atomic": atomic,
                "idempotency_key": format!("d-{account}-{atomic}"),
            })),
        ))
        .await
        .expect("deposit request handled");
    assert_eq!(response.status(), StatusCode::CREATED);
}

async fn reserve_and_start(app: axum::Router, account: &str, key: &str) -> String {
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/reservations".to_string(),
            Some(json!({
                "account_id": account,
                "sku": "scan.v1",
                "amount_atomic": "1000000",
                "idempotency_key": key,
            })),
        ))
        .await
        .expect("reservation request handled");
    assert_eq!(response.status(), StatusCode::CREATED);
    let reservation = body_json(response).await;
    let job_id = reservation["job_id"].as_str().expect("job_id").to_string();
    let start = app
        .oneshot(json_request("POST", format!("/jobs/{job_id}/start"), None))
        .await
        .expect("start request handled");
    assert_eq!(start.status(), StatusCode::OK);
    job_id
}

async fn get_json(app: axum::Router, uri: String) -> Value {
    let response = app
        .oneshot(json_request("GET", uri, None))
        .await
        .expect("GET handled");
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await
}

#[tokio::test]
async fn scan_settles_against_report_hash() {
    let state = AppState::new();
    let app = router(state.clone());
    deposit(app.clone(), "acct-1", "5000000").await;
    let job_id = reserve_and_start(app.clone(), "acct-1", "r-1").await;

    let scan = app
        .clone()
        .oneshot(json_request(
            "POST",
            format!("/jobs/{job_id}/scan"),
            Some(strong_envelope()),
        ))
        .await
        .expect("scan request handled");
    assert_eq!(scan.status(), StatusCode::CREATED);
    let body = body_json(scan).await;

    assert_eq!(
        body["settlement"]["charged"]["atomic"].as_str().unwrap(),
        "1000000"
    );
    assert_eq!(body["report"]["decision"].as_str().unwrap(), "supported");
    let hash = body["report"]["content_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 64);
    assert_eq!(
        body["settlement"]["report_hash_hex"].as_str().unwrap(),
        hash
    );

    let job = get_json(app.clone(), format!("/jobs/{job_id}")).await;
    assert_eq!(job["state"].as_str().unwrap(), "settled");

    let account = get_json(app, "/accounts/acct-1".to_string()).await;
    assert_eq!(
        account["settled_total"]["atomic"].as_str().unwrap(),
        "1000000"
    );
    assert_eq!(account["available"]["atomic"].as_str().unwrap(), "4000000");
}

#[tokio::test]
async fn invalid_envelope_is_400_and_keeps_the_job_uncharged() {
    let state = AppState::new();
    let app = router(state.clone());
    deposit(app.clone(), "acct-2", "5000000").await;
    let job_id = reserve_and_start(app.clone(), "acct-2", "r-2").await;

    let mut envelope = strong_envelope();
    envelope["title"] = json!("x".repeat(201));
    let scan = app
        .clone()
        .oneshot(json_request(
            "POST",
            format!("/jobs/{job_id}/scan"),
            Some(envelope),
        ))
        .await
        .expect("scan request handled");
    assert_eq!(scan.status(), StatusCode::BAD_REQUEST);
    let error = body_json(scan).await;
    assert_eq!(error["error"]["code"].as_str().unwrap(), "validation_error");

    let job = get_json(app.clone(), format!("/jobs/{job_id}")).await;
    assert_eq!(job["state"].as_str().unwrap(), "running");

    let account = get_json(app, "/accounts/acct-2".to_string()).await;
    assert_eq!(account["reserved"]["atomic"].as_str().unwrap(), "1000000");
    assert_eq!(account["settled_total"]["atomic"].as_str().unwrap(), "0");
}

#[tokio::test]
async fn scan_after_settlement_conflicts() {
    let state = AppState::new();
    let app = router(state.clone());
    deposit(app.clone(), "acct-3", "5000000").await;
    let job_id = reserve_and_start(app.clone(), "acct-3", "r-3").await;

    let first = app
        .clone()
        .oneshot(json_request(
            "POST",
            format!("/jobs/{job_id}/scan"),
            Some(strong_envelope()),
        ))
        .await
        .expect("first scan handled");
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = app
        .oneshot(json_request(
            "POST",
            format!("/jobs/{job_id}/scan"),
            Some(strong_envelope()),
        ))
        .await
        .expect("second scan handled");
    assert_eq!(second.status(), StatusCode::CONFLICT);
    let error = body_json(second).await;
    assert_eq!(
        error["error"]["code"].as_str().unwrap(),
        "illegal_transition"
    );
}

#[tokio::test]
async fn scan_requires_running_state() {
    let state = AppState::new();
    let app = router(state.clone());
    deposit(app.clone(), "acct-4", "5000000").await;

    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/reservations".to_string(),
            Some(json!({
                "account_id": "acct-4",
                "sku": "scan.v1",
                "amount_atomic": "1000000",
                "idempotency_key": "r-4",
            })),
        ))
        .await
        .expect("reservation handled");
    assert_eq!(response.status(), StatusCode::CREATED);
    let job_id = body_json(response).await["job_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Reserved, not started: the scan must refuse.
    let scan = app
        .oneshot(json_request(
            "POST",
            format!("/jobs/{job_id}/scan"),
            Some(strong_envelope()),
        ))
        .await
        .expect("scan handled");
    assert_eq!(scan.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn scanner_failure_is_500_and_the_reservation_is_refundable() {
    let launcher = ScanLauncher::new(
        "definitely-not-a-python-interpreter",
        PathBuf::from("scanner"),
        Duration::from_secs(5),
    );
    let state = AppState::with_launcher("dev-agent", launcher);
    let app = router(state.clone());
    deposit(app.clone(), "acct-5", "5000000").await;
    let job_id = reserve_and_start(app.clone(), "acct-5", "r-5").await;

    let scan = app
        .clone()
        .oneshot(json_request(
            "POST",
            format!("/jobs/{job_id}/scan"),
            Some(strong_envelope()),
        ))
        .await
        .expect("scan handled");
    assert_eq!(scan.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let refund = app
        .clone()
        .oneshot(json_request(
            "POST",
            format!("/jobs/{job_id}/refunds"),
            Some(json!({"reason": "scanner unavailable"})),
        ))
        .await
        .expect("refund handled");
    assert_eq!(refund.status(), StatusCode::OK);

    let account = get_json(app, "/accounts/acct-5".to_string()).await;
    assert_eq!(account["available"]["atomic"].as_str().unwrap(), "5000000");
    assert_eq!(account["reserved"]["atomic"].as_str().unwrap(), "0");
}
