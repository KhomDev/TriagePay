//! Route handlers.
//!
//! Handlers never unwrap or expect: every fallible step returns an
//! `ApiError` with the fixed status mapping. JSON bodies are accepted as
//! `Result<Json<T>, JsonRejection>` so malformed JSON, wrong field types,
//! and unknown fields become a 400 in the standard error shape instead of
//! axum's plain-text rejection.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::dto::{
    parse_amount_atomic, BalancesJson, DepositReceiptJson, DepositRequest, HealthResponse,
};
use crate::error::ApiError;
use crate::AppState;

/// `GET /health` — liveness probe.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// `GET /accounts/{account_id}` — current balances, or 404.
pub async fn get_account(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<BalancesJson>, ApiError> {
    let ledger = state.read_ledger().await;
    let balances = ledger.balances(&account_id)?;
    Ok(Json(BalancesJson::from(balances)))
}

/// `POST /accounts/{account_id}/deposits` — idempotent credit deposit.
///
/// 201 when the deposit is new, 200 when the idempotency key replays the
/// recorded receipt. No funds move on replay.
pub async fn deposit(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    body: Result<Json<DepositRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<DepositReceiptJson>), ApiError> {
    let Json(request) = body.map_err(rejected)?;
    let amount = parse_amount_atomic(&request.amount_atomic)?;
    let mut ledger = state.write_ledger().await;
    let receipt = ledger.deposit(
        state.caller(),
        &account_id,
        amount,
        &request.idempotency_key,
    )?;
    let status = if receipt.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(DepositReceiptJson::from(receipt))))
}

/// Map axum's JSON rejection to the standard 400 error shape.
fn rejected(rejection: JsonRejection) -> ApiError {
    ApiError::validation(format!(
        "request body must be strict JSON: {}",
        rejection.body_text()
    ))
}
