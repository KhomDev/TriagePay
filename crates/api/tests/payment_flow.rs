//! Payment-intent flow: credit funds only from provider observations.
//!
//! These tests prove the anti-spoofing contract end to end: an intent
//! plus polling can fund an account, but only through a provider-observed
//! confirmation that reconciles exactly. Mismatched payments, unconfirmed
//! intents, and replays never move credit.

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

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is JSON")
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: String,
    body: Option<Value>,
) -> axum::response::Response {
    app.oneshot(json_request(method, uri, body))
        .await
        .expect("request handled")
}

async fn create_intent(
    app: axum::Router,
    account: &str,
    amount_atomic: &str,
    scenario: Option<Value>,
) -> Value {
    let mut body = json!({
        "account_id": account,
        "asset_id": "asset.usdc.test",
        "amount_atomic": amount_atomic,
        "expires_in_secs": 3600,
    });
    if let Some(scenario) = scenario {
        body["mock_scenario"] = scenario;
    }
    let response = send(app, "POST", "/payment/intents".to_string(), Some(body)).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    body_json(response).await
}

#[tokio::test]
async fn confirmation_funds_the_account_exactly_once() {
    let state = AppState::new();
    let app = router(state.clone());
    let intent = create_intent(app.clone(), "acct-1", "5000000", None).await;
    let intent_id = intent["intent_id"].as_str().unwrap().to_string();
    assert_eq!(intent["status"].as_str().unwrap(), "pending");
    assert!(intent["deposit_address"]
        .as_str()
        .unwrap()
        .starts_with("mock-deposit-"));

    let first = body_json(
        send(
            app.clone(),
            "POST",
            format!("/payment/intents/{intent_id}/observations"),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(first["reconciled"].as_str().unwrap(), "fund");
    assert!(first["funded"].as_bool().unwrap());
    assert_eq!(
        first["deposit"]["credited"]["atomic"].as_str().unwrap(),
        "5000000"
    );
    assert!(!first["deposit"]["replayed"].as_bool().unwrap());

    let account =
        body_json(send(app.clone(), "GET", "/accounts/acct-1".to_string(), None).await).await;
    assert_eq!(account["available"]["atomic"].as_str().unwrap(), "5000000");
    assert_eq!(
        account["deposited_total"]["atomic"].as_str().unwrap(),
        "5000000"
    );

    // Observing again is a replay: funded, but no second credit.
    let second = body_json(
        send(
            app.clone(),
            "POST",
            format!("/payment/intents/{intent_id}/observations"),
            None,
        )
        .await,
    )
    .await;
    assert!(second["funded"].as_bool().unwrap());
    assert!(second["deposit"]["replayed"].as_bool().unwrap());
    let account_again =
        body_json(send(app, "GET", "/accounts/acct-1".to_string(), None).await).await;
    assert_eq!(
        account_again["deposited_total"]["atomic"].as_str().unwrap(),
        "5000000"
    );
}

#[tokio::test]
async fn mismatched_payment_never_funds() {
    let state = AppState::new();
    let app = router(state.clone());
    let intent = create_intent(
        app.clone(),
        "acct-2",
        "5000000",
        Some(json!({"type": "confirm_wrong_amount", "amount_atomic": "4000000"})),
    )
    .await;
    let intent_id = intent["intent_id"].as_str().unwrap().to_string();

    let observation = body_json(
        send(
            app.clone(),
            "POST",
            format!("/payment/intents/{intent_id}/observations"),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(observation["reconciled"].as_str().unwrap(), "mismatch");
    assert!(!observation["funded"].as_bool().unwrap());
    assert_eq!(
        observation["mismatch"]["expected_amount"]["atomic"]
            .as_str()
            .unwrap(),
        "5000000"
    );
    assert_eq!(
        observation["mismatch"]["observed_amount"]["atomic"]
            .as_str()
            .unwrap(),
        "4000000"
    );

    // The account was never created: no funding path ever ran.
    let missing = send(app, "GET", "/accounts/acct-2".to_string(), None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unconfirmed_intent_stays_pending_without_funding() {
    let state = AppState::new();
    let app = router(state.clone());
    let intent = create_intent(
        app.clone(),
        "acct-3",
        "1000000",
        Some(json!({"type": "never_confirm"})),
    )
    .await;
    let intent_id = intent["intent_id"].as_str().unwrap().to_string();

    let observation = body_json(
        send(
            app.clone(),
            "POST",
            format!("/payment/intents/{intent_id}/observations"),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(observation["reconciled"].as_str().unwrap(), "still_pending");
    assert!(!observation["funded"].as_bool().unwrap());

    let missing = send(app, "GET", "/accounts/acct-3".to_string(), None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn refund_is_one_shot() {
    let state = AppState::new();
    let app = router(state.clone());
    let intent = create_intent(app.clone(), "acct-4", "2000000", None).await;
    let intent_id = intent["intent_id"].as_str().unwrap().to_string();

    // Refund requires confirmation first.
    let too_early = body_json(
        send(
            app.clone(),
            "POST",
            format!("/payment/intents/{intent_id}/refunds"),
            Some(json!({"reason": "too early"})),
        )
        .await,
    )
    .await;
    assert_eq!(
        too_early["error"]["code"].as_str().unwrap(),
        "provider_state"
    );

    let _ = body_json(
        send(
            app.clone(),
            "POST",
            format!("/payment/intents/{intent_id}/observations"),
            None,
        )
        .await,
    )
    .await;

    let refund = body_json(
        send(
            app.clone(),
            "POST",
            format!("/payment/intents/{intent_id}/refunds"),
            Some(json!({"reason": "wrong rail"})),
        )
        .await,
    )
    .await;
    assert_eq!(refund["amount"]["atomic"].as_str().unwrap(), "2000000");
    assert_eq!(refund["reason"].as_str().unwrap(), "wrong rail");

    let second = send(
        app,
        "POST",
        format!("/payment/intents/{intent_id}/refunds"),
        Some(json!({"reason": "again"})),
    )
    .await;
    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn unknown_intent_is_404() {
    let state = AppState::new();
    let app = router(state);
    let ghost = "intent_0000000000ff";
    let fetch = send(
        app.clone(),
        "GET",
        format!("/payment/intents/{ghost}"),
        None,
    )
    .await;
    assert_eq!(fetch.status(), StatusCode::NOT_FOUND);
    let observe = send(
        app,
        "POST",
        format!("/payment/intents/{ghost}/observations"),
        None,
    )
    .await;
    assert_eq!(observe.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn zero_amount_intent_is_rejected() {
    let state = AppState::new();
    let app = router(state);
    let response = send(
        app,
        "POST",
        "/payment/intents".to_string(),
        Some(json!({
            "account_id": "acct-6",
            "asset_id": "asset.usdc.test",
            "amount_atomic": "0",
            "expires_in_secs": 3600,
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
