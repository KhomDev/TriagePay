//! Stable error codes for policy refusals.
//!
//! The code strings are part of the public interface: the wallet contract
//! panics with them, explorers display them, and clients match on them.
//! They never change meaning once published; new failure modes get new
//! codes.

use std::fmt;

/// Spend would push the window total past `daily_max`.
pub const ERR_DAILY_LIMIT: &str = "ERR_DAILY_LIMIT";
/// A single payment is larger than `per_call_max`.
pub const ERR_PER_CALL_LIMIT: &str = "ERR_PER_CALL_LIMIT";
/// The receiver is not on the agent's payee allowlist.
pub const ERR_PAYEE_NOT_ALLOWED: &str = "ERR_PAYEE_NOT_ALLOWED";
/// The token contract is not on the wallet's token allowlist.
pub const ERR_TOKEN_NOT_ALLOWED: &str = "ERR_TOKEN_NOT_ALLOWED";
/// The wallet is paused by its owner.
pub const ERR_PAUSED: &str = "ERR_PAUSED";
/// The agent is unknown or has been revoked.
pub const ERR_AGENT_INACTIVE: &str = "ERR_AGENT_INACTIVE";
/// A payment amount of zero was requested.
pub const ERR_ZERO_AMOUNT: &str = "ERR_ZERO_AMOUNT";
/// The policy parameters are inconsistent (see [`PolicyError::InvalidPolicy`]).
pub const ERR_INVALID_POLICY: &str = "ERR_INVALID_POLICY";
/// A u128 or u64 operation would overflow.
pub const ERR_OVERFLOW: &str = "ERR_OVERFLOW";
/// A release asked to return more than the window currently holds.
pub const ERR_RELEASE_MISMATCH: &str = "ERR_RELEASE_MISMATCH";

/// Every refusal the policy core can produce. No function in this crate
/// panics on caller input; all fallible operations return one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyError {
    /// See [`ERR_DAILY_LIMIT`].
    DailyLimit,
    /// See [`ERR_PER_CALL_LIMIT`].
    PerCallLimit,
    /// See [`ERR_PAYEE_NOT_ALLOWED`].
    PayeeNotAllowed,
    /// See [`ERR_TOKEN_NOT_ALLOWED`].
    TokenNotAllowed,
    /// See [`ERR_PAUSED`].
    Paused,
    /// See [`ERR_AGENT_INACTIVE`].
    AgentInactive,
    /// See [`ERR_ZERO_AMOUNT`].
    ZeroAmount,
    /// `per_call_max` is zero, `daily_max` is below `per_call_max`, or the
    /// payee list exceeds [`crate::MAX_PAYEES`]. See [`ERR_INVALID_POLICY`].
    InvalidPolicy,
    /// See [`ERR_OVERFLOW`].
    Overflow,
    /// See [`ERR_RELEASE_MISMATCH`].
    ReleaseMismatch,
}

impl PolicyError {
    /// The stable error code string for this refusal.
    pub const fn code(self) -> &'static str {
        match self {
            PolicyError::DailyLimit => ERR_DAILY_LIMIT,
            PolicyError::PerCallLimit => ERR_PER_CALL_LIMIT,
            PolicyError::PayeeNotAllowed => ERR_PAYEE_NOT_ALLOWED,
            PolicyError::TokenNotAllowed => ERR_TOKEN_NOT_ALLOWED,
            PolicyError::Paused => ERR_PAUSED,
            PolicyError::AgentInactive => ERR_AGENT_INACTIVE,
            PolicyError::ZeroAmount => ERR_ZERO_AMOUNT,
            PolicyError::InvalidPolicy => ERR_INVALID_POLICY,
            PolicyError::Overflow => ERR_OVERFLOW,
            PolicyError::ReleaseMismatch => ERR_RELEASE_MISMATCH,
        }
    }

    /// A short human-readable explanation that follows the code.
    pub const fn description(self) -> &'static str {
        match self {
            PolicyError::DailyLimit => "payment would exceed the daily limit",
            PolicyError::PerCallLimit => "payment exceeds the per-call limit",
            PolicyError::PayeeNotAllowed => "receiver is not on the payee allowlist",
            PolicyError::TokenNotAllowed => "token is not on the token allowlist",
            PolicyError::Paused => "wallet is paused",
            PolicyError::AgentInactive => "agent is unknown or revoked",
            PolicyError::ZeroAmount => "amount must be greater than zero",
            PolicyError::InvalidPolicy => "policy parameters are inconsistent",
            PolicyError::Overflow => "arithmetic overflow",
            PolicyError::ReleaseMismatch => "release exceeds the reserved window total",
        }
    }
}

impl fmt::Display for PolicyError {
    /// Formats as `CODE: description`, so the stable code is always the
    /// prefix of the panic message.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.description())
    }
}

impl std::error::Error for PolicyError {}
