//! Concurrency integration tests for the HTTP API.
//!
//! Every test drives genuinely concurrent requests through the one-shot
//! router (one ledger per test, cloned per request) and asserts the
//! invariants that make the threat model's "API concurrency tests" row
//! literally true: races on the same credit are serialized by the
//! single-writer ledger, idempotency collapses concurrent duplicates into
//! exactly one money movement, and concurrent scans of one job settle
//! exactly once. No test sleeps or depends on wall-clock timing: every
//! outcome is forced by the ledger's own atomicity, not by delays.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::task::JoinSet;
use tower::ServiceExt;

use aps_api::{router, AppState};

/// One USDC (6 decimals) in atomic units.
const ONE_UNIT: &str = "1000000";

fn app() -> axum::Router {
    router(AppState::new())
}

async fn send(app: axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.oneshot(request).await.expect("router dispatch");
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

async fn post(app: axum::Router, uri: String, body: Value) -> (StatusCode, Value) {
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

async fn post_empty(app: axum::Router, uri: String) -> (StatusCode, Value) {
    send(
        app,
        Request::builder()
            .method("POST")
            .uri(uri)
            .body(Body::empty())
            .expect("POST request"),
    )
    .await
}

async fn get(app: axum::Router, uri: String) -> (StatusCode, Value) {
    send(
        app,
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("GET request"),
    )
    .await
}

async fn fund(app: axum::Router, account: &str, atomic: &str, key: &str) {
    let (status, body) = post(
        app,
        format!("/accounts/{account}/deposits"),
        json!({"amount_atomic": atomic, "idempotency_key": key}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "deposit failed: {body}");
}

/// Reserve one USDC for a fresh job and move it to Running.
async fn running_job(app: axum::Router, account: &str, key: &str) -> String {
    let (status, body) = post(
        app.clone(),
        "/reservations".to_string(),
        json!({
            "account_id": account,
            "sku": "scan.v1",
            "amount_atomic": ONE_UNIT,
            "idempotency_key": key,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "reservation failed: {body}");
    let job_id = body["job_id"].as_str().expect("job id").to_string();
    let (status, body) = post_empty(app, format!("/jobs/{job_id}/start")).await;
    assert_eq!(status, StatusCode::OK, "start failed: {body}");
    job_id
}

async fn balances(app: axum::Router, account: &str) -> Value {
    let (status, body) = get(app, format!("/accounts/{account}")).await;
    assert_eq!(status, StatusCode::OK, "balances read: {body}");
    body
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

/// A valid finding envelope, shaped like the scanner-discovery fixtures
/// in `scan_flow.rs`.
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

/// Ten concurrent reservations race for five units of credit: the ledger
/// must let exactly five win, refuse the other five with 422, and leave
/// the account fully conserved.
#[tokio::test]
async fn reservation_race_locks_exactly_the_funded_credit() {
    let app = app();
    fund(app.clone(), "acct-race", "5000000", "dep-race").await;

    let mut race = JoinSet::new();
    for i in 0..10u32 {
        let app = app.clone();
        race.spawn(async move {
            post(
                app,
                "/reservations".to_string(),
                json!({
                    "account_id": "acct-race",
                    "sku": "scan.v1",
                    "amount_atomic": ONE_UNIT,
                    "idempotency_key": format!("res-race-{i}"),
                }),
            )
            .await
        });
    }

    let mut created: Vec<Value> = Vec::new();
    let mut refused = 0usize;
    while let Some(joined) = race.join_next().await {
        let (status, body) = joined.expect("reservation task must not panic");
        match status {
            StatusCode::CREATED => {
                assert_eq!(body["replayed"], json!(false), "fresh winner: {body}");
                assert_eq!(atomic_of(&body, "reserved"), ONE_UNIT, "{body}");
                created.push(body);
            }
            StatusCode::UNPROCESSABLE_ENTITY => {
                assert_eq!(error_code(&body), "insufficient_funds", "{body}");
                refused += 1;
            }
            other => panic!("unexpected race status {other}: {body}"),
        }
    }
    assert_eq!(
        created.len(),
        5,
        "exactly five reservations may lock the credit"
    );
    assert_eq!(refused, 5, "exactly five reservations must be refused");

    // The winners are five distinct jobs, not one job replayed.
    let job_ids: Vec<&str> = created
        .iter()
        .map(|body| body["job_id"].as_str().expect("job id"))
        .collect();
    let distinct: std::collections::HashSet<&str> = job_ids.iter().copied().collect();
    assert_eq!(distinct.len(), 5, "each winning key reserves its own job");

    let final_balances = balances(app, "acct-race").await;
    assert_eq!(atomic_of(&final_balances, "available"), "0");
    assert_eq!(atomic_of(&final_balances, "reserved"), "5000000");
    assert_eq!(atomic_of(&final_balances, "deposited_total"), "5000000");
    assert_eq!(atomic_of(&final_balances, "settled_total"), "0");
    assert_eq!(atomic_of(&final_balances, "refunded_total"), "0");
}

/// Eight concurrent polls of one payment intent: every poll answers 200
/// and may report the funding receipt, but the ledger credits the intent
/// amount exactly once.
#[tokio::test]
async fn concurrent_double_poll_funding_credits_exactly_once() {
    let app = app();
    let (status, intent) = post(
        app.clone(),
        "/payment/intents".to_string(),
        json!({
            "account_id": "acct-poll",
            "asset_id": "asset.usdc.test",
            "amount_atomic": ONE_UNIT,
            "expires_in_secs": 3600,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{intent}");
    let intent_id = intent["intent_id"].as_str().expect("intent id").to_string();

    let mut polls = JoinSet::new();
    for _ in 0..8u32 {
        let app = app.clone();
        let uri = format!("/payment/intents/{intent_id}/observations");
        polls.spawn(async move { post_empty(app, uri).await });
    }

    let mut fresh_credits = 0usize;
    while let Some(joined) = polls.join_next().await {
        let (status, body) = joined.expect("poll task must not panic");
        assert_eq!(status, StatusCode::OK, "every poll answers 200: {body}");
        assert!(
            body["funded"].as_bool().unwrap_or(false),
            "a confirmed intent funds every poll observation: {body}"
        );
        let deposit = &body["deposit"];
        assert_eq!(
            atomic_of(deposit, "credited"),
            ONE_UNIT,
            "replays return the recorded receipt: {body}"
        );
        if deposit["replayed"].as_bool() == Some(false) {
            fresh_credits += 1;
        }
    }
    assert_eq!(fresh_credits, 1, "exactly one poll may move money");

    let final_balances = balances(app, "acct-poll").await;
    assert_eq!(
        atomic_of(&final_balances, "deposited_total"),
        ONE_UNIT,
        "the intent amount is credited exactly once"
    );
    assert_eq!(atomic_of(&final_balances, "available"), ONE_UNIT);
    assert_eq!(atomic_of(&final_balances, "reserved"), "0");
    assert_eq!(atomic_of(&final_balances, "settled_total"), "0");
}

/// Eight concurrent deposits sharing one idempotency key and payload:
/// one writer wins, the other seven replay the recorded receipt, and the
/// account is credited exactly once.
#[tokio::test]
async fn concurrent_same_key_deposit_credits_exactly_once() {
    let app = app();

    let mut deposits = JoinSet::new();
    for _ in 0..8u32 {
        let app = app.clone();
        let uri = "/accounts/acct-samekey/deposits".to_string();
        let key = "dep-samekey".to_string();
        deposits.spawn(async move {
            post(
                app,
                uri,
                json!({"amount_atomic": "2000000", "idempotency_key": key}),
            )
            .await
        });
    }

    let mut created = 0usize;
    let mut replayed = 0usize;
    while let Some(joined) = deposits.join_next().await {
        let (status, body) = joined.expect("deposit task must not panic");
        match status {
            StatusCode::CREATED => {
                assert_eq!(body["replayed"], json!(false), "one writer wins: {body}");
                created += 1;
            }
            StatusCode::OK => {
                assert_eq!(body["replayed"], json!(true), "the rest replay: {body}");
                assert_eq!(
                    atomic_of(&body, "credited"),
                    "2000000",
                    "replay returns the recorded receipt: {body}"
                );
                replayed += 1;
            }
            other => panic!("unexpected deposit status {other}: {body}"),
        }
    }
    assert_eq!(created, 1, "exactly one request creates the deposit");
    assert_eq!(replayed, 7, "the other seven replay it");

    let final_balances = balances(app, "acct-samekey").await;
    assert_eq!(atomic_of(&final_balances, "deposited_total"), "2000000");
    assert_eq!(atomic_of(&final_balances, "available"), "2000000");
    assert_eq!(atomic_of(&final_balances, "reserved"), "0");
}

/// Two concurrent scans of one Running job: both may complete the scanner
/// run, but exactly one settles. The loser either replays the first
/// settlement (200, no money moved) or is cleanly rejected (409). The
/// account is never charged twice.
#[tokio::test]
async fn concurrent_scan_settles_exactly_once() {
    let app = app();
    fund(app.clone(), "acct-scan", "5000000", "dep-scan").await;
    let job_id = running_job(app.clone(), "acct-scan", "res-scan").await;

    let mut scans = JoinSet::new();
    for _ in 0..2u32 {
        let app = app.clone();
        let uri = format!("/jobs/{job_id}/scan");
        let envelope = strong_envelope();
        scans.spawn(async move { post(app, uri, envelope).await });
    }

    let mut outcomes: Vec<(StatusCode, Value)> = Vec::new();
    while let Some(joined) = scans.join_next().await {
        outcomes.push(joined.expect("scan task must not panic"));
    }

    let mut settled_body: Option<Value> = None;
    let mut loser: Option<(StatusCode, Value)> = None;
    for (status, body) in outcomes {
        if status == StatusCode::CREATED {
            assert!(
                settled_body.is_none(),
                "only one scan may create the settlement"
            );
            settled_body = Some(body);
        } else {
            assert!(loser.is_none(), "exactly one losing scan outcome");
            loser = Some((status, body));
        }
    }
    let settled_body = settled_body.expect("one scan must settle");
    let (other_status, other_body) = loser.expect("the second scan must answer");

    let settlement = &settled_body["settlement"];
    assert_eq!(
        settlement["charged"]["atomic"].as_str(),
        Some(ONE_UNIT),
        "the winning scan charges exactly one unit: {settled_body}"
    );
    assert_eq!(settlement["replayed"], json!(false));
    let report_hash = settled_body["report"]["content_hash"]
        .as_str()
        .expect("content hash")
        .to_string();
    assert_eq!(
        settlement["report_hash_hex"].as_str(),
        Some(report_hash.as_str())
    );

    match other_status {
        StatusCode::OK => {
            assert_eq!(
                other_body["settlement"]["replayed"],
                json!(true),
                "the second scan replays the first settlement: {other_body}"
            );
            assert_eq!(
                other_body["settlement"]["report_hash_hex"].as_str(),
                Some(report_hash.as_str()),
                "a replay settles against the same report hash: {other_body}"
            );
        }
        StatusCode::CONFLICT => {
            assert_eq!(
                error_code(&other_body),
                "illegal_transition",
                "the second scan is cleanly rejected: {other_body}"
            );
        }
        status => panic!("unexpected second-scan status {status}: {other_body}"),
    }

    // No double charge: exactly one unit left the reservation.
    let (status, snapshot) = get(app.clone(), format!("/jobs/{job_id}")).await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["state"].as_str(), Some("settled"));

    let final_balances = balances(app, "acct-scan").await;
    assert_eq!(atomic_of(&final_balances, "settled_total"), ONE_UNIT);
    assert_eq!(atomic_of(&final_balances, "reserved"), "0");
    assert_eq!(atomic_of(&final_balances, "available"), "4000000");
    assert_eq!(atomic_of(&final_balances, "deposited_total"), "5000000");
    assert_eq!(atomic_of(&final_balances, "refunded_total"), "0");
}
