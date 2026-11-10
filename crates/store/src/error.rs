//! Typed store errors. No panics on any open or mutation path.

use std::fmt;

use aps_core::CoreError;

/// Every failure the store can produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The ledger core rejected the operation. Nothing was journaled.
    Core(CoreError),
    /// The backing database failed.
    Backend(String),
    /// The hash chain does not verify at this sequence number.
    ChainBroken { seq: u64, reason: String },
    /// A row decoded to something that is not a valid event.
    Corrupt { seq: u64, reason: String },
    /// Replaying an event produced a different outcome than the one
    /// recorded when it was first applied.
    ReplayDivergence { seq: u64, reason: String },
    /// A snapshot does not match the verified chain.
    SnapshotMismatch { seq: u64, reason: String },
    /// The replayed state violates conservation.
    ConservationViolation(String),
    /// The projection and the replayed core ledger disagree.
    ProjectionMismatch(String),
    /// An API-level idempotency key was reused with a different request.
    IdempotencyConflict(String),
    /// A payment-intent state change is not a legal transition.
    IllegalIntentTransition {
        intent: String,
        from: String,
        to: String,
    },
    /// A journal write failed after the in-memory state changed. The
    /// handle refuses all further work until it is reopened.
    Poisoned,
    /// Input failed validation.
    Validation(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StoreError::Core(err) => write!(f, "ledger: {err}"),
            StoreError::Backend(msg) => write!(f, "store backend: {msg}"),
            StoreError::ChainBroken { seq, reason } => {
                write!(f, "journal hash chain broken at seq {seq}: {reason}")
            }
            StoreError::Corrupt { seq, reason } => {
                write!(f, "journal row {seq} is corrupt: {reason}")
            }
            StoreError::ReplayDivergence { seq, reason } => {
                write!(f, "replay diverged at seq {seq}: {reason}")
            }
            StoreError::SnapshotMismatch { seq, reason } => {
                write!(
                    f,
                    "snapshot at seq {seq} does not match the journal: {reason}"
                )
            }
            StoreError::ConservationViolation(msg) => {
                write!(f, "conservation violated after replay: {msg}")
            }
            StoreError::ProjectionMismatch(msg) => {
                write!(f, "projection disagrees with the replayed ledger: {msg}")
            }
            StoreError::IdempotencyConflict(key) => {
                write!(f, "idempotency key reused with a different request: {key}")
            }
            StoreError::IllegalIntentTransition { intent, from, to } => {
                write!(f, "illegal intent transition for {intent}: {from} -> {to}")
            }
            StoreError::Poisoned => write!(
                f,
                "a journal write failed after the state changed; reopen the store"
            ),
            StoreError::Validation(msg) => write!(f, "validation error: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<CoreError> for StoreError {
    fn from(err: CoreError) -> Self {
        StoreError::Core(err)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        StoreError::Backend(err.to_string())
    }
}
