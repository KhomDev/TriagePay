//! The single API error model.
//!
//! Every failure crosses the wire in one shape:
//!
//! ```json
//! {"error":{"code":"...","message":"..."}}
//! ```
//!
//! Status mapping (fixed by the route contract):
//!
//! | core error            | code                   | status |
//! |-----------------------|------------------------|--------|
//! | `Validation`           | `validation_error`      | 400    |
//! | `UnknownAccount`       | `unknown_account`      | 404    |
//! | `UnknownJob`           | `unknown_job`          | 404    |
//! | `IdempotencyConflict`  | `idempotency_conflict` | 409    |
//! | `IllegalTransition`    | `illegal_transition`   | 409    |
//! | `InsufficientFunds`    | `insufficient_funds`   | 422    |
//! | `Overflow`             | `overflow`             | 500    |
//!
//! Malformed JSON bodies, unknown request fields, and non-numeric amount
//! strings are all `validation_error` at 400. Job-registry failures and
//! other unexpected conditions surface as `internal` at 500 without
//! detail.

use aps_core::CoreError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// An API failure carrying the wire error code and HTTP status.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    /// 400: the request failed schema or value validation.
    pub(crate) fn validation(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "validation_error", message)
    }

    /// 404: no account with this id was ever funded.
    pub(crate) fn unknown_account(account: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "unknown_account",
            format!("unknown account: {account}"),
        )
    }

    /// 404: no job with this id exists in this ledger.
    pub(crate) fn unknown_job(job: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "unknown_job",
            format!("unknown job: {job}"),
        )
    }

    /// 409: the request conflicts with recorded state.
    pub(crate) fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    /// 422: the account exists but cannot cover the operation.
    pub(crate) fn insufficient_funds(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "insufficient_funds",
            message,
        )
    }

    /// 500: an unexpected internal condition.
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message)
    }
}

impl From<CoreError> for ApiError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::Validation(_) => Self::validation(err.to_string()),
            CoreError::UnknownAccount(account) => Self::unknown_account(&account),
            CoreError::UnknownJob(job) => Self::unknown_job(&job),
            CoreError::IdempotencyConflict(_) => {
                Self::conflict("idempotency_conflict", err.to_string())
            }
            CoreError::IllegalTransition { .. } => {
                Self::conflict("illegal_transition", err.to_string())
            }
            CoreError::InsufficientFunds { .. } => Self::insufficient_funds(err.to_string()),
            CoreError::Overflow => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "overflow",
                err.to_string(),
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "error": { "code": self.code, "message": self.message }
        }));
        (self.status, body).into_response()
    }
}
