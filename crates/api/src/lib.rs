//! AgentPay Sentinel HTTP API: an axum 0.8 surface over the
//! dependency-free prepaid ledger in `aps-core`.
//!
//! Wire contract:
//! - all money crosses the boundary as u128 **atomic units encoded as JSON
//!   strings**; receipts additionally render a display-only decimal string;
//! - every money-moving request carries a caller-scoped idempotency key;
//!   replay returns the recorded receipt and moves no funds;
//! - every failure is one error shape, `{"error":{"code","message"}}`.
//!
//! Documented limitations of this milestone:
//! - a single dev caller acts for every request (`AppState::DEV_CALLER`);
//!   real authentication is a later milestone;
//! - the ledger is in-memory and process-local; durability is a later
//!   milestone.

mod dto;
mod error;
mod handlers;
mod scan;
pub mod scanbridge;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use aps_core::{JobId, Ledger};

pub use error::ApiError;

/// The one caller this milestone trusts for every request.
pub const DEV_CALLER: &str = "dev-agent";

/// Shared application state behind every handler.
///
/// The ledger sits behind a `tokio::sync::RwLock` (single-writer by
/// construction, matching the core's design). The state is cheaply
/// cloneable so axum can route with it; the `Arc` exists only because
/// axum requires `Clone` state.
#[derive(Clone)]
pub struct AppState {
    ledger: Arc<tokio::sync::RwLock<Ledger>>,
    /// Wire job id to the canonical core `JobId`.
    ///
    /// `aps-core` creates job ids only inside `reserve` and keeps `JobId`
    /// construction crate-private, so the API records the mapping when a
    /// reservation first returns it. The registry is therefore exactly the
    /// set of jobs the ledger knows: a wire id that was never returned by
    /// `reserve` is answered `unknown_job` at 404, matching the ledger's
    /// own view. A public `FromStr` for `JobId` in the core would let
    /// this field disappear.
    job_ids: Arc<RwLock<HashMap<String, JobId>>>,
    caller: String,
    /// Scanner subprocess launcher (see `scanbridge`).
    scan: Arc<scanbridge::ScanLauncher>,
}

impl AppState {
    /// State with the default dev caller and an empty ledger.
    pub fn new() -> Self {
        Self::with_caller(DEV_CALLER)
    }

    /// State with an explicit caller, for future authentication
    /// milestones and for tests that need caller isolation.
    pub fn with_caller(caller: impl Into<String>) -> Self {
        Self {
            ledger: Arc::new(tokio::sync::RwLock::new(Ledger::new())),
            job_ids: Arc::new(RwLock::new(HashMap::new())),
            caller: caller.into(),
            scan: Arc::new(scanbridge::ScanLauncher::discover()),
        }
    }

    /// State with an explicit scanner launcher, for tests and controlled
    /// environments.
    pub fn with_launcher(caller: impl Into<String>, launcher: scanbridge::ScanLauncher) -> Self {
        Self {
            ledger: Arc::new(tokio::sync::RwLock::new(Ledger::new())),
            job_ids: Arc::new(RwLock::new(HashMap::new())),
            caller: caller.into(),
            scan: Arc::new(launcher),
        }
    }

    /// The caller identity every request acts under.
    pub fn caller(&self) -> &str {
        &self.caller
    }

    /// The scanner subprocess launcher.
    pub(crate) fn scan_launcher(&self) -> &scanbridge::ScanLauncher {
        &self.scan
    }

    /// Read lock on the ledger for query handlers. Tokio's async
    /// `RwLock` is not poisonable, so acquisition cannot fail.
    pub(crate) async fn read_ledger(&self) -> tokio::sync::RwLockReadGuard<'_, Ledger> {
        self.ledger.read().await
    }

    /// Write lock on the ledger for money-moving handlers.
    pub(crate) async fn write_ledger(&self) -> tokio::sync::RwLockWriteGuard<'_, Ledger> {
        self.ledger.write().await
    }

    /// Record a job id returned by a successful reservation.
    ///
    /// Called with the ledger lock held. The registry lock is a brief,
    /// synchronous critical section that never awaits and is never held
    /// while acquiring the ledger, so no lock cycle can form.
    pub(crate) fn register_job(&self, job_id: &JobId) -> Result<(), ApiError> {
        self.job_ids
            .write()
            .map_err(|_| ApiError::internal("job registry is unavailable"))?
            .insert(job_id.to_string(), job_id.clone());
        Ok(())
    }

    /// Look up the canonical `JobId` for a wire job id string.
    pub(crate) fn resolve_job(&self, wire_id: &str) -> Result<Option<JobId>, ApiError> {
        Ok(self
            .job_ids
            .read()
            .map_err(|_| ApiError::internal("job registry is unavailable"))?
            .get(wire_id)
            .cloned())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the application router over the given state.
pub fn router(state: AppState) -> axum::Router {
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/health", get(handlers::health))
        .route("/accounts/{account_id}", get(handlers::get_account))
        .route("/accounts/{account_id}/deposits", post(handlers::deposit))
        .route("/reservations", post(handlers::create_reservation))
        .route("/jobs/{job_id}", get(handlers::get_job))
        .route("/jobs/{job_id}/start", post(handlers::start_job))
        .route("/jobs/{job_id}/scan", post(scan::scan_job))
        .route("/jobs/{job_id}/settlements", post(handlers::settle_job))
        .route("/jobs/{job_id}/refunds", post(handlers::refund_job))
        .with_state(state)
}
