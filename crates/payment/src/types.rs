//! Intent, observation, and error types for payment providers.

use aps_core::Amount;
use std::fmt;

/// Opaque deposit-intent identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IntentId(String);

impl IntentId {
    /// Construct an intent identifier. Exposed because tests and gateway
    /// code need to name intents they learned about out of band.
    pub fn new(id: String) -> Self {
        IntentId(id)
    }

    /// The identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IntentId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Asset identifier as used by the provider (for example a token id on
/// the settlement network). An opaque string; never a bare symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId(String);

impl AssetId {
    /// Construct an asset identifier.
    pub fn new(id: impl Into<String>) -> Self {
        AssetId(id.into())
    }

    /// The identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Request to create a deposit intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateIntent {
    /// Asset the caller intends to pay with.
    pub asset_id: AssetId,
    /// Amount the caller intends to deposit, atomic units.
    pub amount: Amount,
    /// Absolute expiry, unix seconds. The provider refuses intents that
    /// are already expired at creation time.
    pub expires_at_unix: u64,
}

/// A deposit intent issued by a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositIntent {
    pub intent_id: IntentId,
    /// Address the caller must pay to.
    pub deposit_address: String,
    pub asset_id: AssetId,
    pub amount: Amount,
    pub expires_at_unix: u64,
    pub status: IntentStatus,
}

/// Provider-side status of an intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentStatus {
    /// Awaiting payment or confirmation.
    Pending,
    /// Payment observed and finalized by the provider.
    Confirmed { tx_ref: String },
    /// The intent expired without confirmation.
    Expired,
    /// The provider refused the intent.
    Rejected { reason: String },
}

/// What the provider observed on a poll. Observations are idempotent:
/// polling a confirmed intent returns the same confirmation forever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    /// Nothing final yet.
    Pending,
    /// A payment matching the intent was confirmed by the provider.
    Confirmed {
        tx_ref: String,
        asset_id: AssetId,
        amount: Amount,
    },
    /// The intent expired unconfirmed.
    Expired,
    /// The provider rejected the intent.
    Rejected { reason: String },
}

/// A refund instruction issued by the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundRequest {
    pub refund_id: String,
    pub intent_id: IntentId,
    pub amount: Amount,
    pub reason: String,
}

/// Provider failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// The intent id is unknown to this provider.
    UnknownIntent(String),
    /// The intent was already refunded.
    AlreadyRefunded(String),
    /// The provider is temporarily unavailable (retry later).
    ProviderUnavailable,
    /// Input failed validation.
    Validation(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProviderError::UnknownIntent(id) => write!(f, "unknown intent: {}", id),
            ProviderError::AlreadyRefunded(id) => write!(f, "intent already refunded: {}", id),
            ProviderError::ProviderUnavailable => write!(f, "provider temporarily unavailable"),
            ProviderError::Validation(msg) => write!(f, "validation error: {}", msg),
        }
    }
}

impl std::error::Error for ProviderError {}

/// The provider contract. Implementations must be deterministic under
/// test; real adapters translate this trait onto their protocol.
pub trait PaymentProvider {
    /// Create a deposit intent for the requested asset and amount.
    fn create_intent(&mut self, req: CreateIntent) -> Result<DepositIntent, ProviderError>;

    /// Observe the current state of an intent. Observing is read-only
    /// and idempotent; it never moves funds by itself.
    fn observe(&mut self, intent_id: &IntentId) -> Result<Observation, ProviderError>;

    /// Request a refund of a confirmed intent. Refunding twice is an
    /// error, not a second payout.
    fn request_refund(
        &mut self,
        intent_id: &IntentId,
        reason: &str,
    ) -> Result<RefundRequest, ProviderError>;
}
