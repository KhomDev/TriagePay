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

use std::sync::Arc;

use aps_core::Ledger;

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
    caller: String,
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
            caller: caller.into(),
        }
    }

    /// The caller identity every request acts under.
    pub fn caller(&self) -> &str {
        &self.caller
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
        .with_state(state)
}
