//! `POST /jobs/{job_id}/scan`: execute the paid scan and settle.
//!
//! Contract:
//! - the job must exist and be in the Running state (409 otherwise);
//! - the request body is the finding envelope, forwarded verbatim to the
//!   scanner subprocess: the scanner is the schema authority;
//! - a completed report settles the job against the report content hash.
//!   Payment covers the scan, not the verdict: a rejected or
//!   needs-evidence report settles exactly like a supported one;
//! - an invalid envelope is a 400 and the job stays Running, uncharged;
//!   the caller can fix the envelope and retry, or request a refund;
//! - a scanner infrastructure failure is a 500 and the job stays Running;
//!   the reservation is refundable.
//!
//! The scan runs without the ledger lock held; two concurrent scans of
//! one job both complete, and the second settlement is rejected by the
//! ledger as an illegal transition (409) without double charging.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

use aps_core::{JobState, ReportHash};

use crate::dto::SettlementJson;
use crate::error::ApiError;
use crate::handlers::known_job;
use crate::scanbridge::{ScanBridgeError, ScanOutcome};
use crate::AppState;

/// Execute the paid scan for one Running job and settle against the
/// report content hash.
pub async fn scan_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let raw = std::str::from_utf8(&body)
        .map_err(|_| ApiError::validation("request body must be UTF-8 JSON"))?;
    let probe: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| ApiError::validation("request body must be valid JSON"))?;
    if !probe.is_object() {
        return Err(ApiError::validation("request body must be a JSON object"));
    }

    let job = known_job(&state, &job_id)?;
    {
        let ledger = state.read_ledger().await;
        match ledger.job_state(&job) {
            Err(err) => return Err(ApiError::from(err)),
            Ok(JobState::Running) => {}
            Ok(other) => {
                return Err(ApiError::conflict(
                    "illegal_transition",
                    format!("scan requires the job to be running, but it is {other}"),
                ))
            }
        }
    }

    let outcome = state
        .scan_launcher()
        .launch(raw)
        .await
        .map_err(bridge_error)?;
    let (report, content_hash) = match outcome {
        ScanOutcome::Report {
            report,
            content_hash,
        } => (report, content_hash),
        ScanOutcome::InvalidInput { errors } => {
            // Bound every message and the joined total: scanner errors are
            // trusted only up to their fixed vocabulary, and a hostile
            // payload must never be reflected into an HTTP body.
            let joined = if errors.is_empty() {
                "the envelope failed validation".to_string()
            } else {
                errors
                    .iter()
                    .map(|error| truncate_at_char_boundary(error, 120))
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            let joined = truncate_at_char_boundary(&joined, 500);
            return Err(ApiError::validation(format!(
                "envelope failed validation: {joined}"
            )));
        }
    };

    let hash = ReportHash::from_hex(&content_hash)
        .map_err(|_| ApiError::internal("scanner produced an invalid report hash"))?;
    let settlement = {
        let mut ledger = state.write_ledger().await;
        ledger.settle(&job, hash)?
    };
    let status = if settlement.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        Json(json!({
            "settlement": SettlementJson::from(settlement),
            "report": report,
        })),
    ))
}

/// The largest prefix of a string that does not exceed the byte limit and
/// ends on a character boundary.
fn truncate_at_char_boundary(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn bridge_error(err: ScanBridgeError) -> ApiError {
    let message = match err {
        ScanBridgeError::Launch(message) => format!("scanner could not start: {message}"),
        ScanBridgeError::Timeout { seconds } => {
            format!("scanner timed out after {seconds}s; the reservation is refundable")
        }
        ScanBridgeError::MalformedOutput(message) => {
            format!("scanner output malformed: {message}")
        }
        ScanBridgeError::ScannerFailed { exit_code, stderr } => {
            format!("scanner failed (exit {exit_code:?}): {stderr}")
        }
    };
    ApiError::internal(message)
}
