//! Spend-policy core for the policy wallet contract.
//!
//! This crate holds the policy arithmetic the wallet contract enforces on
//! chain, with no `near-sdk` dependency, so an off-chain simulator and the
//! property tests run the exact same rules.
//!
//! Design rules enforced in this crate:
//! - integer money only, u128 atomic units, checked math, no floats;
//! - a spend window is one UTC day derived from a nanosecond timestamp;
//! - a payment is checked and reserved in one step, and a refused payment
//!   leaves the policy unchanged;
//! - a release restores the exact pre-reservation total, and a release that
//!   arrives after its window rolled over changes nothing;
//! - every refusal carries a stable error code string.

#![warn(clippy::pedantic)]
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::similar_names
)]

pub mod error;
pub mod hold;
pub mod policy;
pub mod window;

pub use error::{
    PolicyError, ERR_AGENT_DAILY_HOLDS, ERR_AGENT_INACTIVE, ERR_AGENT_OPEN_HOLDS, ERR_DAILY_LIMIT,
    ERR_INVALID_POLICY, ERR_OVERFLOW, ERR_PAUSED, ERR_PAYEE_NOT_ALLOWED, ERR_PER_CALL_LIMIT,
    ERR_RELEASE_MISMATCH, ERR_TOKEN_NOT_ALLOWED, ERR_ZERO_AMOUNT,
};
pub use hold::{HoldQuota, MAX_DAILY_HOLDS_PER_AGENT, MAX_OPEN_HOLDS_PER_AGENT};
pub use policy::{AgentPolicy, ReleaseOutcome, Reservation, WalletGate, MAX_PAYEES};
pub use window::{next_window_start, window_index, NANOS_PER_DAY};
