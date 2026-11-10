//! Payment provider abstraction for TriagePay.
//!
//! The gateway funds credit exclusively from provider-observed
//! confirmations; a client assertion of payment is never sufficient.
//! This crate defines the provider trait, the intent and observation
//! types, the pure reconciliation logic that decides whether an
//! observation funds an intent, and a deterministic mock provider that
//! models finality delay, mismatched payments, expiry, and refunds.

pub mod mock;
pub mod reconcile;
pub mod types;

pub use mock::{MockClock, MockProvider, MockScenario};
pub use reconcile::{reconcile, ReconcileOutcome};
pub use types::{
    AssetId, CreateIntent, DepositIntent, IntentId, IntentStatus, Observation, PaymentProvider,
    ProviderError, RefundRequest,
};
