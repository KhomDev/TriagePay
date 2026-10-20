//! Error-hygiene integration tests for the HTTP API.
//!
//! A unique canary token is planted in every string field of a finding
//! envelope and driven through every scan failure path plus the success
//! path. The canary must never appear in any response body: no error
//! message may reflect hostile envelope content, and a successful report
//! must stay limited to its public fields. These tests make the threat
//! model's "log assertions" verification row literally true at the
//! response-body boundary, and pin the uniform error shape for 404s and
//! the zero-value rejections.

use std::path::PathBuf;
use std::time::Duration;

use aps_api::scanbridge::ScanLauncher;
use aps_api::{router, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

/// One USDC (6 decimals) in atomic units.
const ONE_UNIT: &str = "1000000";

/// A unique canary token planted in every envelope string field. If it
/// ever shows up in a response body, an error path is reflecting hostile
/// input.
const CANARY: &str = "CANARY7Q4X9Z";

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

async fn fund(app: &axum::Router, account: &str, atomic: &str, key: &str) {
    let (status, body) = post(
        app,
        &format!("/accounts/{account}/deposits"),
        json!({"amount_atomic": atomic, "idempotency_key": key}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "deposit failed: {body}");
}

/// Reserve one USDC, start the job, and return its id.
async fn running_job(app: &axum::Router, account: &str, key: &str) -> String {
    let job_id = reserved_job(app, account, key).await;
    let (status, body) = post(app, &format!("/jobs/{job_id}/start"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "start failed: {body}");
    job_id
}

/// Reserve one USDC and leave the job in the Reserved state.
async fn reserved_job(app: &axum::Router, account: &str, key: &str) -> String {
    let (status, body) = post(
        app,
        "/reservations",
        json!({
            "account_id": account,
            "sku": "scan.v1",
            "amount_atomic": ONE_UNIT,
            "idempotency_key": key,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "reservation failed: {body}");
    body["job_id"].as_str().expect("job id").to_string()
}

async fn balances(app: &axum::Router, account: &str) -> Value {
    let (status, body) = get(app, &format!("/accounts/{account}")).await;
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

/// Assert the canary token (and therefore any envelope field value that
/// carries it) is absent from the whole response body.
fn assert_no_canary(context: &str, body: &Value) {
    let serialized = serde_json::to_string(body).expect("body serializes");
    assert!(
        !serialized.contains(CANARY),
        "{context}: the response reflected envelope content: {serialized}"
    );
}

/// Assert the body is exactly `{"error": {"code": ..., "message": ...}}`
/// with non-empty string values and no extra keys anywhere.
fn assert_uniform_error_shape(body: &Value) {
    let root = body
        .as_object()
        .unwrap_or_else(|| panic!("error body is an object: {body}"));
    assert_eq!(root.len(), 1, "one root key only: {body}");
    let error = body["error"]
        .as_object()
        .unwrap_or_else(|| panic!("error envelope is an object: {body}"));
    let mut keys: Vec<&str> = error.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["code", "message"], "uniform error shape: {body}");
    let code = error["code"]
        .as_str()
        .unwrap_or_else(|| panic!("code is a string: {body}"));
    let message = error["message"]
        .as_str()
        .unwrap_or_else(|| panic!("message is a string: {body}"));
    assert!(!code.is_empty(), "code is non-empty: {body}");
    assert!(!message.is_empty(), "message is non-empty: {body}");
}

/// A canary-bearing string for one envelope field.
fn canary_value(purpose: &str) -> String {
    format!("{CANARY}-{purpose}")
}

/// A schema-valid envelope whose every string field carries the canary.
fn canary_envelope() -> Value {
    json!({
        "title": canary_value("title"),
        "claimed_impact": canary_value("claimed-impact"),
        "mechanism": canary_value("mechanism"),
        "attacker_capability": canary_value("attacker-capability"),
        "preconditions": canary_value("preconditions"),
        "component_class": canary_value("component-class"),
        "scope_statement": canary_value("scope-statement"),
        "deployed_state_evidence": canary_value("deployed-state-evidence"),
        "public_rules_snapshot": canary_value("rules-snapshot"),
        "known_issue_declaration": null,
        "reproduction_summary": canary_value("reproduction-summary"),
        "evidence_refs": [canary_value("evidence-ref")],
        "code_excerpt": canary_value("code-excerpt"),
    })
}

/// Every scan failure path answers with its contracted status and never
/// reflects the canary: neither the over-length rejection (400), the
/// malformed-JSON rejection (400), the non-running-job rejection (409),
/// nor the scanner infrastructure failure (500). The refused scans leave
/// the job uncharged.
#[tokio::test]
async fn scan_error_paths_never_reflect_the_canary() {
    let app = app();
    fund(&app, "acct-hyg", "5000000", "dep-hyg").await;
    let job_id = running_job(&app, "acct-hyg", "res-hyg").await;

    // (i) schema-invalid envelope: an over-length title.
    let mut over_length = canary_envelope();
    over_length["title"] = json!(format!("{CANARY}{}", "x".repeat(200)));
    let (status, body) = post(&app, &format!("/jobs/{job_id}/scan"), over_length).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");
    assert_no_canary("over-length envelope rejection", &body);

    // (ii) raw malformed JSON carrying the canary.
    let raw = format!("{{\"title\": \"{CANARY}\", \"claimed_imp\"");
    let (status, body) = post_raw(&app, &format!("/jobs/{job_id}/scan"), &raw).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");
    assert_no_canary("malformed JSON rejection", &body);

    // (iv) a valid envelope on a job that is not Running.
    let idle_job = reserved_job(&app, "acct-hyg", "res-hyg-idle").await;
    let (status, body) = post(&app, &format!("/jobs/{idle_job}/scan"), canary_envelope()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "illegal_transition");
    assert_no_canary("non-running job rejection", &body);

    // (v) scanner infrastructure failure: the interpreter cannot start.
    let launcher = ScanLauncher::new(
        "definitely-not-a-python-interpreter",
        PathBuf::from("scanner"),
        Duration::from_secs(5),
    );
    let failing = router(AppState::with_launcher("dev-agent", launcher));
    fund(&failing, "acct-hyg-fail", "5000000", "dep-hyg-fail").await;
    let failing_job = running_job(&failing, "acct-hyg-fail", "res-hyg-fail").await;
    let (status, body) = post(
        &failing,
        &format!("/jobs/{failing_job}/scan"),
        canary_envelope(),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_no_canary("scanner infrastructure failure", &body);

    // Refused scans never charge: the running job is intact.
    let (status, snapshot) = get(&app, &format!("/jobs/{job_id}")).await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["state"].as_str(), Some("running"));
    let final_balances = balances(&app, "acct-hyg").await;
    assert_eq!(atomic_of(&final_balances, "reserved"), "2000000");
    assert_eq!(atomic_of(&final_balances, "settled_total"), "0");
    assert_eq!(atomic_of(&final_balances, "available"), "3000000");
}

/// An unknown envelope field is rejected with 400 and the job stays
/// Running and uncharged (the scanner is the schema authority).
#[tokio::test]
async fn unknown_envelope_field_is_rejected_as_400() {
    let app = app();
    fund(&app, "acct-unk", "5000000", "dep-unk").await;
    let job_id = running_job(&app, "acct-unk", "res-unk").await;

    let mut envelope = canary_envelope();
    envelope[CANARY] = json!(canary_value("unknown-field"));
    let (status, body) = post(&app, &format!("/jobs/{job_id}/scan"), envelope).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    let (status, snapshot) = get(&app, &format!("/jobs/{job_id}")).await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["state"].as_str(), Some("running"));
    let final_balances = balances(&app, "acct-unk").await;
    assert_eq!(atomic_of(&final_balances, "reserved"), ONE_UNIT);
    assert_eq!(atomic_of(&final_balances, "settled_total"), "0");
}

/// The unknown-field 400 message must not echo the attacker-chosen field
/// name. See the ignore reason for the product defect this documents.
// FIXED (was a product bug found by this suite): the scanner CLI's
// envelope loader echoed unknown field names as
// "unknown envelope fields: <name, ...>", and scan.rs forwarded the
// joined scanner errors verbatim in the 400 message, so an
// attacker-controlled envelope field name was reflected into the
// response body. The loader now reports only the count of unknown
// fields, and scan.rs bounds every message before it reaches a response.
#[tokio::test]
async fn unknown_field_name_is_not_echoed_in_error_body() {
    let app = app();
    fund(&app, "acct-unk-echo", "5000000", "dep-unk-echo").await;
    let job_id = running_job(&app, "acct-unk-echo", "res-unk-echo").await;

    let mut envelope = canary_envelope();
    envelope[CANARY] = json!(canary_value("unknown-field"));
    let (status, body) = post(&app, &format!("/jobs/{job_id}/scan"), envelope).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");
    assert_no_canary("unknown-field rejection", &body);
}

/// A successful scan settles the job and answers with the settlement
/// receipt plus the public report. The report carries exactly its eight
/// public fields — no envelope content, no private labels — and the whole
/// response stays canary-free.
#[tokio::test]
async fn successful_scan_report_is_canary_free_and_public_shaped() {
    let app = app();
    fund(&app, "acct-ok", "5000000", "dep-ok").await;
    let job_id = running_job(&app, "acct-ok", "res-ok").await;

    let (status, body) = post(&app, &format!("/jobs/{job_id}/scan"), canary_envelope()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_no_canary("successful scan response", &body);

    let report = body["report"]
        .as_object()
        .unwrap_or_else(|| panic!("report is an object: {body}"));
    let mut report_keys: Vec<&str> = report.keys().map(String::as_str).collect();
    report_keys.sort_unstable();
    assert_eq!(
        report_keys,
        vec![
            "confidence_band",
            "contains_embedded_instructions",
            "content_hash",
            "decision",
            "failed_gates",
            "gate_results",
            "missing_evidence",
            "ruleset_version",
        ],
        "the report is limited to its public fields: {body}"
    );

    let decision = report["decision"].as_str().expect("decision");
    assert!(
        matches!(
            decision,
            "supported" | "needs_evidence" | "held" | "rejected"
        ),
        "decision is a public verdict: {body}"
    );

    let gates = report["gate_results"].as_array().expect("gate results");
    for gate in gates {
        let mut gate_keys: Vec<&str> = gate
            .as_object()
            .expect("gate entry")
            .keys()
            .map(String::as_str)
            .collect();
        gate_keys.sort_unstable();
        assert_eq!(
            gate_keys,
            vec!["gate", "reason", "status"],
            "gate entry shape: {gate}"
        );
    }

    let content_hash = report["content_hash"].as_str().expect("content hash");
    assert_eq!(content_hash.len(), 64);
    assert!(
        content_hash.chars().all(|c| c.is_ascii_hexdigit()),
        "content hash is 64 hex characters: {body}"
    );
    assert_eq!(
        body["settlement"]["report_hash_hex"].as_str(),
        Some(content_hash),
        "settlement binds the report content hash: {body}"
    );
    assert_eq!(
        body["settlement"]["charged"]["atomic"].as_str(),
        Some(ONE_UNIT),
        "the scan charges exactly one unit: {body}"
    );

    let final_balances = balances(&app, "acct-ok").await;
    assert_eq!(atomic_of(&final_balances, "settled_total"), ONE_UNIT);
    assert_eq!(atomic_of(&final_balances, "reserved"), "0");
}

/// Unknown account and unknown job responses share the uniform
/// `{"error": {"code", "message"}}` shape with no extra keys.
#[tokio::test]
async fn not_found_responses_share_the_uniform_error_shape() {
    let app = app();

    let (status, body) = get(&app, "/accounts/ghost-account").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_uniform_error_shape(&body);
    assert_eq!(error_code(&body), "unknown_account");

    let (status, body) = get(&app, "/jobs/job_0000000000000999").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_uniform_error_shape(&body);
    assert_eq!(error_code(&body), "unknown_job");

    let (status, body) = post(&app, "/jobs/job_0000000000000999/start", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_uniform_error_shape(&body);
    assert_eq!(error_code(&body), "unknown_job");
}

/// A zero-amount reservation is rejected with 400 before any credit is
/// locked.
#[tokio::test]
async fn zero_amount_reservation_is_rejected() {
    let app = app();
    fund(&app, "acct-zero-res", "2000000", "dep-zero-res").await;

    let (status, body) = post(
        &app,
        "/reservations",
        json!({
            "account_id": "acct-zero-res",
            "sku": "scan.v1",
            "amount_atomic": "0",
            "idempotency_key": "res-zero",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    let final_balances = balances(&app, "acct-zero-res").await;
    assert_eq!(atomic_of(&final_balances, "available"), "2000000");
    assert_eq!(atomic_of(&final_balances, "reserved"), "0");
}

/// A settlement whose report hash is 64 zero hex characters is rejected
/// with 400: a settlement must bind to real report content.
#[tokio::test]
async fn zero_report_hash_settlement_is_rejected() {
    let app = app();
    fund(&app, "acct-zero-hash", "2000000", "dep-zero-hash").await;
    let job_id = running_job(&app, "acct-zero-hash", "res-zero-hash").await;

    let (status, body) = post(
        &app,
        &format!("/jobs/{job_id}/settlements"),
        json!({"report_hash_hex": "0".repeat(64)}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "validation_error");

    let (status, snapshot) = get(&app, &format!("/jobs/{job_id}")).await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["state"].as_str(), Some("running"));
    let final_balances = balances(&app, "acct-zero-hash").await;
    assert_eq!(atomic_of(&final_balances, "reserved"), ONE_UNIT);
    assert_eq!(atomic_of(&final_balances, "settled_total"), "0");
}
