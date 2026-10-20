//! Typed errors for the ledger core.

use crate::money::Amount;
use std::fmt;

/// Every failure the ledger core can produce. No panics and no unwrap on
/// request paths: all fallible operations return one of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// An arithmetic operation would overflow the atomic-unit u128.
    Overflow,
    /// A reservation or settlement exceeded spendable credit.
    InsufficientFunds {
        account: String,
        needed: Amount,
        available: Amount,
    },
    /// The account does not exist. Deposits create accounts; reserves do not.
    UnknownAccount(String),
    /// An idempotency key was reused with a different payload.
    IdempotencyConflict(String),
    /// The job id is unknown to this ledger.
    UnknownJob(String),
    /// The requested transition is illegal for the current job state.
    IllegalTransition {
        job: String,
        from: String,
        to: String,
    },
    /// Input failed validation.
    Validation(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CoreError::Overflow => write!(f, "arithmetic overflow in atomic units"),
            CoreError::InsufficientFunds {
                account,
                needed,
                available,
            } => {
                let d = crate::money::SETTLEMENT_DECIMALS;
                write!(
                    f,
                    "insufficient funds for account {}: needed {}, available {}",
                    account,
                    needed.format(d),
                    available.format(d)
                )
            }
            CoreError::UnknownAccount(a) => write!(f, "unknown account: {}", a),
            CoreError::IdempotencyConflict(k) => {
                write!(f, "idempotency key reused with a different payload: {}", k)
            }
            CoreError::UnknownJob(j) => write!(f, "unknown job: {}", j),
            CoreError::IllegalTransition { job, from, to } => {
                write!(f, "illegal transition for job {}: {} -> {}", job, from, to)
            }
            CoreError::Validation(msg) => write!(f, "validation error: {}", msg),
        }
    }
}

impl std::error::Error for CoreError {}
