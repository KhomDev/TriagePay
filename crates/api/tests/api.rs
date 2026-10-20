//! Integration tests for the HTTP API.
//!
//! Every request is driven through `tower::ServiceExt::oneshot` on the
//! real router: no port is ever bound and no network is touched. Each
//! test owns one router (one ledger) and clones it per oneshot call, so
//! state persists across the requests of a test while tests stay
//! isolated from each other. The suite covers the route contract end to
//! end: the happy money path, idempotent replays and conflicts, the
//! error model, and strict request-body handling.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use aps_api::{router, AppState};

/// One USDC (6 decimals) in atomic units.
const ONE_UNIT: &str = "1000000";

fn app() -> axum::Router {
    router(AppState::new())
}

async fn send(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("router dispatch");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body collection")
        .to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("JSON body")
    };
    (status, body)
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    send(
        app,
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("GET request"),
    )
    .await
}

async fn post(app: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    send(
        app,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("POST request"),
    )
    .await
}

async fn post_raw(app: &axum::Router, uri: &str, raw: &str) -> (StatusCode, Value) {
    send(
        app,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(raw.to_string()))
            .expect("raw POST request"),
    )
    .await
}

async fn deposit(
    app: &axum::Router,
    account: &str,
    amount_atomic: &str,
    key: &str,
) -> (StatusCode, Value) {
    post(
        app,
        &format!("/accounts/{account}/deposits"),
        json!({"amount_atomic": amount_atomic, "idempotency_key": key}),
    )
    .await
}

async fn reserve(
    app: &axum::Router,
    account: &str,
    sku: &str,
    amount_atomic: &str,
    key: &str,
) -> (StatusCode, Value) {
    post(
        app,
        "/reservations",
        json!({
            "account_id": account,
            "sku": sku,
            "amount_atomic": amount_atomic,
            "idempotency_key": key,
        }),
    )
    .await
}

async fn balances(app: &axum::Router, account: &str) -> Value {
    let (status, body) = get(app, &format!("/accounts/{account}")).await;
    assert_eq!(status, StatusCode::OK, "account balances read: {body}");
    body
}

/// A valid non-zero 64-hex-character report hash built from a one-byte seed.
fn report_hex(seed: u8) -> String {
    format!("{seed:02x}").repeat(32)
}

/// The atomic-units string of a nested `{atomic, decimal}` field.
fn atomic_of(body: &Value, field: &str) -> String {
    body[field]["atomic"]
        .as_str()
        .unwrap_or_else(|| panic!("field {field} missing atomic units: {body}"))
        .to_string()
}

fn error_code(body: &Value) -> &str {
    body["error"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("no error code: {body}"))
}

#[tokio::test]
async fn health_returns_ok() {
    let app = app();
    let (status, body) = get(&app, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"status": "ok"}));
}

#[tokio::test]
async fn full_happy_path_deposit_reserve_start_settle() {
    let app = app();

    let (status, body) = deposit(&app, "acct-1", "5000000", "dep-1").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(atomic_of(&body, "credited"), "5000000");
    assert_eq!(body["account"].as_str(), Some("acct-1"));
    assert_eq!(body["replayed"], json!(false));
    assert_eq!(atomic_of(&body["balances"], "available"), "5000000");

    let (status, body) = reserve(&app, "acct-1", "scan.v1", ONE_UNIT, "res-1").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let job_id = body["job_id"].as_str().expect("job id").to_string();
    assert!(!job_id.is_empty());
    assert_eq!(atomic_of(&body, "reserved"), ONE_UNIT);
    assert_eq!(atomic_of(&body["balances"], "available"), "4000000");
    assert_eq!(atomic_of(&body["balances"], "reserved"), ONE_UNIT);

    let (status, body) = post(&app, &format!("/jobs/{job_id}/start"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"].as_str(), Some("running"));

    let hash = report_hex(7);
    let (status, body) = post(
        &app,
        &format!("/jobs/{job_id}/settlements"),
        json!({"report_hash_hex": hash}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(atomic_of(&body, "charged"), ONE_UNIT);
    assert_eq!(body["report_hash_hex"].as_str(), Some(hash.as_str()));
    assert_eq!(atomic_of(&body["balances"], "settled_total"), ONE_UNIT);
    assert_eq!(atomic_of(&body["balances"], "reserved"), "0");

    let final_balances = balances(&app, "acct-1").await;
    assert_eq!(atomic_of(&final_balances, "available"), "4000000");
    assert_eq!(
        final_balances["available"]["decimal"].as_str(),
        Some("4.000000")
    );
    assert_eq!(atomic_of(&final_balances, "reserved"), "0");
    assert_eq!(atomic_of(&final_balances, "settled_total"), ONE_UNIT);
    assert_eq!(atomic_of(&final_balances, "refunded_total"), "0");
    assert_eq!(atomic_of(&final_balances, "deposited_total"), "5000000");

    let (status, snapshot) = get(&app, &format!("/jobs/{job_id}")).await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["state"].as_str(), Some("settled"));
    assert_eq!(atomic_of(&snapshot, "amount"), ONE_UNIT);
    assert_eq!(
        snapshot["settlement"]["report_hash_hex"].as_str(),
        Some(hash.as_str())
    );
    assert_eq!(snapshot["refund"], Value::Null);
}

#[tokio::test]
async fn deposit_replay_returns_200_without_double_credit() {
    let app = app();
    let (status, _) = deposit(&app, "acct-1", "2000000", "dep-replay").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = deposit(&app, "acct-1", "2000000", "dep-replay").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["replayed"], json!(true));
    assert_eq!(atomic_of(&body, "credited"), "2000000");

    let body = balances(&app, "acct-1").await;
    assert_eq!(atomic_of(&body, "deposited_total"), "2000000");
    assert_eq!(atomic_of(&body, "available"), "2000000");
}

#[tokio::test]
async fn deposit_same_key_different_amount_conflicts() {
    let app = app();
    let (status, _) = deposit(&app, "acct-1", "2000000", "dep-conflict").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = deposit(&app, "acct-1", "3000000", "dep-conflict").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "idempotency_conflict");

    let body = balances(&app, "acct-1").await;
    assert_eq!(atomic_of(&body, "deposited_total"), "2000000");
}

#[tokio::test]
async fn reserve_insufficient_funds_is_422() {
    let app = app();
    let (status, _) = deposit(&app, "acct-1", ONE_UNIT, "dep-funds").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = reserve(&app, "acct-1", "scan.v1", "2000000", "res-funds").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "insufficient_funds");

    let body = balances(&app, "acct-1").await;
    assert_eq!(atomic_of(&body, "available"), ONE_UNIT);
    assert_eq!(atomic_of(&body, "reserved"), "0");
}

#[tokio::test]
async fn unknown_account_and_job_are_404() {
    let app = app();

    let (status, body) = get(&app, "/accounts/ghost").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(error_code(&body), "unknown_account");

    let (status, body) = get(&app, "/jobs/job_0000000000000999").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(error_code(&body), "unknown_job");

    let (status, body) = post(&app, "/jobs/job_0000000000000999/start", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(error_code(&body), "unknown_job");
}

#[tokio::test]
async fn refund_returns_credit_once_and_replays_as_200() {
    let app = app();
    let (status, _) = deposit(&app, "acct-1", "3000000", "dep-refund").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = reserve(&app, "acct-1", "scan.v1", "1500000", "res-refund").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let job_id = body["job_id"].as_str().expect("job id").to_string();

    let (status, body) = post(
        &app,
        &format!("/jobs/{job_id}/refunds"),
        json!({"reason": "tool timeout"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["replayed"], json!(false));
    assert_eq!(atomic_of(&body, "refunded"), "1500000");
    assert_eq!(body["reason"].as_str(), Some("tool timeout"));
    assert_eq!(atomic_of(&body["balances"], "available"), "3000000");
    assert_eq!(atomic_of(&body["balances"], "reserved"), "0");
    assert_eq!(atomic_of(&body["balances"], "refunded_total"), "1500000");

    // Replay with a different reason still replays the original receipt.
    let (status, body) = post(
        &app,
        &format!("/jobs/{job_id}/refunds"),
        json!({"reason": "retry from client"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["replayed"], json!(true));

    let body = balances(&app, "acct-1").await;
    assert_eq!(atomic_of(&body, "available"), "3000000");
    assert_eq!(atomic_of(&body, "refunded_total"), "1500000");

    // A refunded job can never be settled afterwards.
    let (status, body) = post(
        &app,
        &format!("/jobs/{job_id}/settlements"),
        json!({"report_hash_hex": report_hex(9)}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "illegal_transition");
}

#[tokio::test]
async fn settlement_validates_hash_state_and_replay() {
    let app = app();
    let (status, _) = deposit(&app, "acct-1", "5000000", "dep-settle").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = reserve(&app, "acct-1", "scan.v1", ONE_UNIT, "res-settle").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let job_id = body["job_id"].as_str().expect("job id").to_string();

    // Before start: any settle attempt is an illegal transition.
    let (status, body) = post(
        &app,
        &format!("/jobs/{job_id}/settlements"),
        json!({"report_hash_hex": report_hex(1)}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "illegal_transition");

    let (status, _) = post(&app, &format!("/jobs/{job_id}/start"), json!({})).await;
    assert_eq!(status, StatusCode::OK);

    // Malformed or zero hashes never reach the state machine.
    for bad in ["not-a-hex-hash-at-all", "abcd", report_hex(0).as_str()] {
        let (status, body) = post(
            &app,
            &format!("/jobs/{job_id}/settlements"),
            json!({"report_hash_hex": bad}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "hash {bad:?}: {body}");
        assert_eq!(error_code(&body), "validation_error");
    }

    let first_hash = report_hex(5);
    let (status, body) = post(
        &app,
        &format!("/jobs/{job_id}/settlements"),
        json!({"report_hash_hex": first_hash}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["replayed"], json!(false));

    // Same hash: replay with 200, funds move exactly once.
    let (status, body) = post(
        &app,
        &format!("/jobs/{job_id}/settlements"),
        json!({"report_hash_hex": first_hash}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["replayed"], json!(true));

    // Different hash on a settled job: hard conflict.
    let (status, body) = post(
        &app,
        &format!("/jobs/{job_id}/settlements"),
        json!({"report_hash_hex": report_hex(6)}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "illegal_transition");

    let body = balances(&app, "acct-1").await;
    assert_eq!(atomic_of(&body, "settled_total"), ONE_UNIT);
    assert_eq!(atomic_of(&body, "available"), "4000000");
}

#[tokio::test]
async fn reservation_replay_returns_same_job() {
    let app = app();
    let (status, _) = deposit(&app, "acct-1", "2000000", "dep-rreplay").await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, first) = reserve(&app, "acct-1", "scan.v1", ONE_UNIT, "res-rreplay").await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, second) = reserve(&app, "acct-1", "scan.v1", ONE_UNIT, "res-rreplay").await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["replayed"], json!(true));
    assert_eq!(
        second["job_id"].as_str(),
        first["job_id"].as_str(),
        "replay must return the original job"
    );

    let body = balances(&app, "acct-1").await;
    assert_eq!(atomic_of(&body, "reserved"), ONE_UNIT);
}

#[tokio::test]
async fn zero_amount_deposit_is_rejected() {
    let app = app();
    let (status, body) = deposit(&app, "acct-1", "0", "dep-zero").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    let (status, _) = get(&app, "/accounts/acct-1").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "account must not be created");
}

#[tokio::test]
async fn malformed_and_strict_violating_bodies_are_rejected() {
    let app = app();

    // Broken JSON syntax.
    let (status, body) = post_raw(&app, "/accounts/acct-1/deposits", "{ this is not json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    // Unknown field.
    let (status, body) = post_raw(
        &app,
        "/accounts/acct-1/deposits",
        r#"{"amount_atomic":"1","idempotency_key":"k","extra":true}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    // Missing field.
    let (status, body) = post_raw(
        &app,
        "/accounts/acct-1/deposits",
        r#"{"amount_atomic":"1"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    // Amount as a JSON number instead of a string.
    let (status, body) = post_raw(
        &app,
        "/accounts/acct-1/deposits",
        r#"{"amount_atomic":1000000,"idempotency_key":"k"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    // Fractional decimal string instead of integer atomic units.
    let (status, body) = post_raw(
        &app,
        "/accounts/acct-1/deposits",
        r#"{"amount_atomic":"1.5","idempotency_key":"k"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    // Missing fields on the reservation route too.
    let (status, body) = post_raw(&app, "/reservations", r#"{"account_id":"a"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");
}

#[tokio::test]
async fn reserve_on_unknown_account_is_404() {
    let app = app();
    let (status, body) = reserve(&app, "ghost", "scan.v1", ONE_UNIT, "res-ghost").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(error_code(&body), "unknown_account");
}
