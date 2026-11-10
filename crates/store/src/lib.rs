//! TriagePay durable ledger.
//!
//! An append-only event journal with a SHA-256 hash chain, replayed into
//! the dependency-free `aps-core` ledger on open.
//!
//! Design rules:
//! - one event row per successful ledger mutation (deposit, reserve,
//!   start, settle, refund), per payment-intent state change, and per
//!   API-level idempotency record; replays and failed operations write
//!   nothing;
//! - every row carries `hash = SHA-256(domain || prev_hash || seq || kind
//!   || body)`, so an edited, reordered or deleted interior row breaks the
//!   chain and the store refuses to open;
//! - on open the `aps-core` ledger is rebuilt by a full replay through its
//!   public API, and each replayed outcome must match the recorded one;
//! - the store keeps its own serializable projection of the state and
//!   snapshots it every N events; on open the projection is restored from
//!   the newest snapshot plus the tail, then cross-checked against the
//!   replayed `aps-core` ledger. Any divergence, a conservation violation,
//!   or a broken chain refuses to open;
//! - a mutation is applied in memory first and journaled second; if the
//!   journal write fails the in-memory state is ahead of the journal, so
//!   the handle poisons itself and refuses further work until reopened.
//!
//! Limits, stated plainly: the hash chain is unkeyed. It detects
//! corruption and naive edits, but an attacker with write access to the
//! database file can recompute the whole chain. Tail truncation is only
//! detected when a snapshot or an externally recorded head hash lies
//! beyond the truncated end. See `docs/operations/runbook.md`.

pub mod callers;
pub mod chain;
pub mod error;
pub mod event;
pub mod journal;
pub mod persistent;
pub mod projection;
pub mod sqlite;

pub use callers::{CallerRecord, CallerTable, MemoryCallers};
pub use chain::{ChainHead, GENESIS_HASH};
pub use error::StoreError;
pub use event::LedgerEvent;
pub use journal::{LedgerStore, MemoryStore, SnapshotRow, StoredEvent};
pub use persistent::{IdempotencyOutcome, OpenOptions, OpenReport, PersistentLedger};
pub use projection::{
    AccountImage, IdempotencyEntry, IntentImage, IntentState, JobImage, Projection,
};
pub use sqlite::SqliteStore;

/// Lowercase hexadecimal rendering of bytes.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Parse exactly 64 hexadecimal characters into 32 bytes.
pub fn from_hex32(text: &str) -> Option<[u8; 32]> {
    let raw = text.as_bytes();
    if raw.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in raw.chunks(2).enumerate() {
        let hi = hex_val(pair[0])?;
        let lo = hex_val(pair[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// SHA-256 of bytes.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let bytes = sha256(b"abc");
        let text = to_hex(&bytes);
        assert_eq!(
            text,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(from_hex32(&text), Some(bytes));
        assert_eq!(from_hex32("zz"), None);
        assert_eq!(from_hex32(&"g".repeat(64)), None);
    }
}
