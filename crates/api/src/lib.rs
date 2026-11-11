//! TriagePay HTTP API: an axum 0.8 surface over the
//! dependency-free prepaid ledger in aps-core.
//!
//! Wire contract:
//! - all money crosses the boundary as u128 **atomic units encoded as JSON
//!   strings**; receipts additionally render a display-only decimal string;
//! - every money-moving request carries a caller-scoped idempotency key;
//!   replay returns the recorded receipt and moves no funds;
//! - every failure is one error shape, {"error":{"code","message"}}.
//!
//! Documented limitations of this milestone:
//! - a single dev caller acts for every request (AppState::DEV_CALLER);
//!   real authentication is a later milestone;
//! - the ledger is in-memory and process-local; durability is a later
//!   milestone;
//! - the payment provider is the deterministic mock by default; the NEAR
//!   adapter (verification-only, on-chain deposit verification) activates
//!   only through explicit environment configuration.

mod dto;
mod error;
mod handlers;
pub mod payment;
mod scan;
pub mod scanbridge;
pub mod x402;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use aps_core::{JobId, Ledger};
use aps_payment::{MockClock, MockProvider};
use payment::Provider;

pub use error::ApiError;

/// The one caller this milestone trusts for every request.
pub const DEV_CALLER: &str = "dev-agent";

/// Shared application state behind every handler.
///
/// The ledger sits behind a tokio::sync::RwLock (single-writer by
/// construction, matching the core design). The state is cheaply
/// cloneable so axum can route with it; the Arc exists only because
/// axum requires Clone state.
#[derive(Clone)]
pub struct AppState {
    ledger: Arc<tokio::sync::RwLock<Ledger>>,
    /// Wire job id to the canonical core JobId.
    ///
    /// aps-core creates job ids only inside reserve and keeps JobId
    /// construction crate-private, so the API records the mapping when a
    /// reservation first returns it. The registry is therefore exactly the
    /// set of jobs the ledger knows: a wire id that was never returned by
    /// reserve is answered unknown_job at 404, matching the ledger's
    /// own view. A public FromStr for JobId in the core would let
    /// this field disappear.
    job_ids: Arc<RwLock<HashMap<String, JobId>>>,
    caller: String,
    /// Which payment provider is active: "mock" or "near". Stored
    /// un-locked so kind checks never contend on the provider mutex.
    provider_kind: &'static str,
    /// Scanner subprocess launcher (see scanbridge).
    scan: Arc<scanbridge::ScanLauncher>,
    /// Payment provider. Single-writer behind an async mutex.
    payment: Arc<tokio::sync::Mutex<Provider>>,
    /// Gateway-level link from payment intent to the account it funds.
    intent_accounts: payment::IntentAccounts,
    /// x402 transport state: issued requirements and claimed proofs.
    x402: Arc<x402::X402State>,
}

impl AppState {
    /// State with the default dev caller and an empty ledger.
    pub fn new() -> Self {
        Self::with_caller(DEV_CALLER)
    }

    /// State with an explicit caller, for future authentication
    /// milestones and for tests that need caller isolation.
    pub fn with_caller(caller: impl Into<String>) -> Self {
        let provider = Provider::Mock(MockProvider::new(MockClock::new(unix_now())));
        Self::with_provider(caller, provider)
    }

    /// State with an explicit payment provider. The scanner launcher is
    /// discovered from the environment.
    pub fn with_provider(caller: impl Into<String>, provider: Provider) -> Self {
        Self::with_provider_and_launcher(caller, provider, scanbridge::ScanLauncher::discover())
    }

    /// State with an explicit provider and scanner launcher, for tests
    /// and controlled environments.
    pub fn with_provider_and_launcher(
        caller: impl Into<String>,
        provider: Provider,
        launcher: scanbridge::ScanLauncher,
    ) -> Self {
        let (network, asset, _) = provider.x402_identity();
        let price = aps_core::PriceBook::default_book()
            .price(aps_core::SCAN_SKU)
            .unwrap_or(aps_core::Amount::from_atomic(1_000_000));
        let x402 = x402::X402State::new(x402::X402Config {
            network,
            asset,
            price,
            max_timeout_secs: x402::DEFAULT_MAX_TIMEOUT_SECS,
            prune_interval_secs: x402::PRUNE_INTERVAL_SECS,
        });
        Self {
            x402: Arc::new(x402),
            ledger: Arc::new(tokio::sync::RwLock::new(Ledger::new())),
            job_ids: Arc::new(RwLock::new(HashMap::new())),
            caller: caller.into(),
            provider_kind: provider.kind(),
            scan: Arc::new(launcher),
            payment: Arc::new(tokio::sync::Mutex::new(provider)),
            intent_accounts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Compatibility constructor with an explicit scanner launcher.
    pub fn with_launcher(caller: impl Into<String>, launcher: scanbridge::ScanLauncher) -> Self {
        let provider = Provider::Mock(MockProvider::new(MockClock::new(unix_now())));
        Self::with_provider_and_launcher(caller, provider, launcher)
    }

    /// The caller identity every request acts under.
    pub fn caller(&self) -> &str {
        &self.caller
    }

    /// Override the x402 price of one scan and the requirement validity
    /// window. Call before the state is shared with the router.
    pub fn with_x402_terms(mut self, price: aps_core::Amount, max_timeout_secs: u64) -> Self {
        let mut config = self.x402.config().clone();
        config.price = price;
        config.max_timeout_secs = max_timeout_secs.max(1);
        self.x402 = Arc::new(x402::X402State::new(config));
        self
    }

    /// Override how often expired, unpaid requirements are swept. Call
    /// before the state is shared with the router.
    pub fn with_x402_prune_interval(mut self, secs: u64) -> Self {
        let mut config = self.x402.config().clone();
        config.prune_interval_secs = secs;
        self.x402 = Arc::new(x402::X402State::new(config));
        self
    }

    /// The x402 transport state.
    pub fn x402(&self) -> &x402::X402State {
        &self.x402
    }

    /// The active payment provider kind: "mock" or "near".
    pub fn provider_kind(&self) -> &'static str {
        self.provider_kind
    }

    /// Whether the development-only routes answer: the direct credit
    /// deposit and the settlement against a caller-supplied hash. Both
    /// move ledger state on the caller's word alone, so they exist only
    /// with the mock provider. With a provider that settles real
    /// payments, credit enters an account only from a provider-observed
    /// confirmation, and a job settles only against the hash of a report
    /// the scanner produced.
    pub(crate) fn dev_routes_enabled(&self) -> bool {
        self.provider_kind == "mock"
    }

    /// The scanner subprocess launcher.
    pub(crate) fn scan_launcher(&self) -> &scanbridge::ScanLauncher {
        &self.scan
    }

    /// The payment provider behind its single-writer mutex.
    pub(crate) fn payment(&self) -> &Arc<tokio::sync::Mutex<Provider>> {
        &self.payment
    }

    /// Run one read-style operation against the payment provider under
    /// its lock, with the provider clock synced to wall time.
    pub(crate) async fn with_payment<T>(&self, read: impl FnOnce(&mut Provider) -> T) -> T {
        let mut provider = self.payment.lock().await;
        provider.sync_clock(unix_now());
        read(&mut provider)
    }

    /// Record which account a payment intent funds.
    pub(crate) fn register_intent(
        &self,
        intent_id: &aps_payment::IntentId,
        account_id: &str,
    ) -> Result<(), ApiError> {
        self.intent_accounts
            .write()
            .map_err(|_| ApiError::internal("intent registry is unavailable"))?
            .insert(intent_id.to_string(), account_id.to_string());
        Ok(())
    }

    /// Drop the account links of intents the provider has forgotten.
    pub(crate) fn forget_intents(&self, intent_ids: &[aps_payment::IntentId]) {
        if let Ok(mut accounts) = self.intent_accounts.write() {
            for id in intent_ids {
                accounts.remove(id.as_str());
            }
        }
    }

    /// Look up the account a payment intent funds, 404 when unknown.
    pub(crate) fn intent_account(&self, intent_id: &str) -> Result<String, ApiError> {
        self.intent_accounts
            .read()
            .map_err(|_| ApiError::internal("intent registry is unavailable"))?
            .get(intent_id)
            .cloned()
            .ok_or_else(|| ApiError::unknown_intent(intent_id))
    }

    /// Read lock on the ledger for query handlers. Tokio async RwLock
    /// is not poisonable, so acquisition cannot fail.
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

    /// Look up the canonical JobId for a wire job id string.
    pub(crate) fn resolve_job(&self, wire_id: &str) -> Result<Option<JobId>, ApiError> {
        Ok(self
            .job_ids
            .read()
            .map_err(|_| ApiError::internal("job registry is unavailable"))?
            .get(wire_id)
            .cloned())
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
        .route("/payment/intents", post(payment::create_intent))
        .route("/payment/intents/{intent_id}", get(payment::get_intent))
        .route(
            "/payment/intents/{intent_id}/observations",
            post(payment::observe_intent),
        )
        .route(
            "/payment/intents/{intent_id}/refunds",
            post(payment::refund_intent),
        )
        .route("/jobs/{job_id}/settlements", post(handlers::settle_job))
        .route("/jobs/{job_id}/refunds", post(handlers::refund_job))
        .route(x402::RESOURCE_PATH, post(x402::scan_v2))
        .route("/x402/supported", get(x402::supported))
        .route("/x402/verify", post(x402::verify))
        .route("/x402/settle", post(x402::settle))
        .with_state(state)
}
