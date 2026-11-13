//! Route handlers.
//!
//! Handlers never unwrap or expect: every fallible step returns an
//! `ApiError` with the fixed status mapping. JSON bodies are accepted as
//! `Result<Json<T>, JsonRejection>` so malformed JSON, wrong field types,
//! and unknown fields become a 400 in the standard error shape instead of
//! axum's plain-text rejection.

use aps_core::{JobId, ReportHash};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::dto::{
    parse_amount_atomic, BalancesJson, DepositReceiptJson, DepositRequest, HealthResponse,
    JobSnapshotJson, JobStateJson, RefundReceiptJson, RefundRequest, ReservationJson,
    ReservationRequest, SettlementJson, SettlementRequest,
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
    state.check_account(&account_id)?;
    let ledger = state.read_ledger().await;
    let balances = ledger.balances(&account_id)?;
    Ok(Json(BalancesJson::from(balances)))
}

/// `POST /accounts/{account_id}/deposits` — idempotent credit deposit.
///
/// 201 when the deposit is new, 200 when the idempotency key replays the
/// recorded receipt. No funds move on replay.
///
/// Development only: the credit rests on the caller's word, so the route
/// answers 403 `dev_only_route` unless the mock provider is active.
pub async fn deposit(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    body: Result<Json<DepositRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<DepositReceiptJson>), ApiError> {
    if !state.dev_routes_enabled() {
        return Err(ApiError::dev_only_route());
    }
    let Json(request) = body.map_err(rejected_json)?;
    let amount = parse_amount_atomic(&request.amount_atomic)?;
    state.claim_account(&account_id)?;
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
///
/// The message names only the kind of failure. serde's own text quotes
/// unknown field names, which are attacker-chosen, so it never reaches the
/// response body (the same rule the scanner errors follow).
pub(crate) fn rejected_json(rejection: JsonRejection) -> ApiError {
    let kind = match &rejection {
        JsonRejection::MissingJsonContentType(_) => "the content type must be application/json",
        JsonRejection::JsonSyntaxError(_) => "the body is not valid JSON",
        JsonRejection::JsonDataError(_) => "a field is missing, unknown, or of the wrong type",
        JsonRejection::BytesRejection(_) => "the body could not be read",
        _ => "the body was rejected",
    };
    ApiError::validation(format!("request body must be strict JSON: {kind}"))
}

/// Resolve a wire job id to the canonical core `JobId`, 404 if unknown.
pub(crate) fn known_job(state: &AppState, wire_id: &str) -> Result<JobId, ApiError> {
    state
        .resolve_job(wire_id)?
        .ok_or_else(|| ApiError::unknown_job(wire_id))
}

/// `POST /reservations` — lock available credit for a job.
///
/// The server prices the reservation from its price book: the SKU must be
/// the one this server's scan route runs, and `amount_atomic`, when sent,
/// must equal that price (the mock provider also accepts another non-zero
/// amount as a development override). The account must belong to the
/// caller, and the job is bound to the caller that reserved it.
///
/// 201 when the reservation is new, 200 when the idempotency key replays
/// the recorded receipt. No funds move on replay.
pub async fn create_reservation(
    State(state): State<AppState>,
    body: Result<Json<ReservationRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ReservationJson>), ApiError> {
    let Json(request) = body.map_err(rejected_json)?;
    let requested = request
        .amount_atomic
        .as_deref()
        .map(parse_amount_atomic)
        .transpose()?;
    let amount = state.reservation_price(&request.sku, requested)?;
    state.check_account(&request.account_id)?;
    let mut ledger = state.write_ledger().await;
    let reservation = ledger.reserve(
        state.caller(),
        &request.account_id,
        &request.sku,
        amount,
        &request.idempotency_key,
    )?;
    // Record the job id before the response leaves the ledger lock; the
    // registry lock never awaits and never wraps a ledger acquisition.
    state.register_job(&reservation.job_id)?;
    let status = if reservation.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(ReservationJson::from(reservation))))
}

/// `GET /jobs/{job_id}` — the job snapshot, or 404.
pub async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobSnapshotJson>, ApiError> {
    let job_id = known_job(&state, &job_id)?;
    let ledger = state.read_ledger().await;
    let snapshot = ledger.job_snapshot(&job_id)?;
    Ok(Json(JobSnapshotJson::from(snapshot)))
}

/// `POST /jobs/{job_id}/start` — move a Reserved job to Running.
///
/// Idempotent: starting a Running job again reports `running` without
/// side effects.
pub async fn start_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStateJson>, ApiError> {
    let job_id = known_job(&state, &job_id)?;
    let mut ledger = state.write_ledger().await;
    let job_state = ledger.start(&job_id)?;
    Ok(Json(JobStateJson {
        job_id: job_id.to_string(),
        state: job_state.to_string(),
    }))
}

/// `POST /jobs/{job_id}/settlements` — consume reserved credit against
/// a non-zero report hash.
///
/// 201 when the settlement is new, 200 when the same hash replays the
/// recorded receipt, 409 for a different hash on a settled job.
///
/// Development only: the hash is the caller's, not a report's, so the
/// route answers 403 `dev_only_route` unless the mock provider is active.
/// `POST /jobs/{job_id}/scan` and `POST /v2/scan` settle against the hash
/// of the report the scanner produced.
pub async fn settle_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    body: Result<Json<SettlementRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<SettlementJson>), ApiError> {
    if !state.dev_routes_enabled() {
        return Err(ApiError::dev_only_route());
    }
    let Json(request) = body.map_err(rejected_json)?;
    // Validate the hash before resolving the job: bad input outranks
    // unknown references.
    let report_hash = ReportHash::from_hex(&request.report_hash_hex)?;
    let job_id = known_job(&state, &job_id)?;
    let mut ledger = state.write_ledger().await;
    let settlement = ledger.settle(&job_id, report_hash)?;
    let status = if settlement.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(SettlementJson::from(settlement))))
}

/// `POST /jobs/{job_id}/refunds` — return reserved credit to available.
///
/// 200 always; a second refund replays the recorded receipt and returns
/// the credit exactly once.
pub async fn refund_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    body: Result<Json<RefundRequest>, JsonRejection>,
) -> Result<Json<RefundReceiptJson>, ApiError> {
    let Json(request) = body.map_err(rejected_json)?;
    let job_id = known_job(&state, &job_id)?;
    let mut ledger = state.write_ledger().await;
    let receipt = ledger.refund(&job_id, &request.reason)?;
    Ok(Json(RefundReceiptJson::from(receipt)))
}
