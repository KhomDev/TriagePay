//! The SHA-256 hash chain over journal rows.
//!
//! `hash(n) = SHA-256("aps-journal-v1" || prev_hash || seq_be64 ||
//! len_be32(kind) || kind || len_be32(body) || body)`, with the genesis
//! `prev_hash` all zero. Length prefixes keep the encoding unambiguous.

use crate::error::StoreError;
use crate::journal::StoredEvent;

/// Domain separator for journal row hashes.
pub const CHAIN_DOMAIN: &[u8] = b"aps-journal-v1";

/// The `prev_hash` of the first row.
pub const GENESIS_HASH: [u8; 32] = [0u8; 32];

/// The last verified position of the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainHead {
    /// Sequence number of the last row; 0 for an empty journal.
    pub seq: u64,
    /// Hash of the last row; the genesis hash for an empty journal.
    pub hash: [u8; 32],
}

impl ChainHead {
    /// The head of an empty journal.
    pub const fn genesis() -> Self {
        ChainHead {
            seq: 0,
            hash: GENESIS_HASH,
        }
    }

    /// The head as lowercase hex, for operators to record off-host.
    pub fn hash_hex(&self) -> String {
        crate::to_hex(&self.hash)
    }
}

/// Compute the hash of one row.
pub fn row_hash(prev_hash: &[u8; 32], seq: u64, kind: &str, body: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(CHAIN_DOMAIN);
    hasher.update(prev_hash);
    hasher.update(seq.to_be_bytes());
    hasher.update((kind.len() as u32).to_be_bytes());
    hasher.update(kind.as_bytes());
    hasher.update((body.len() as u32).to_be_bytes());
    hasher.update(body.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&hasher.finalize());
    out
}

/// Build the next row after `head`.
pub fn next_row(head: &ChainHead, kind: &str, body: String) -> StoredEvent {
    let seq = head.seq + 1;
    let hash = row_hash(&head.hash, seq, kind, &body);
    StoredEvent {
        seq,
        kind: kind.to_string(),
        body,
        prev_hash: head.hash,
        hash,
    }
}

/// Verify a full journal in order, from genesis. Returns the head and,
/// for every row, its hash (index `seq - 1`).
pub fn verify(rows: &[StoredEvent]) -> Result<ChainHead, StoreError> {
    let mut head = ChainHead::genesis();
    for row in rows {
        let expected_seq = head.seq + 1;
        if row.seq != expected_seq {
            return Err(StoreError::ChainBroken {
                seq: row.seq,
                reason: format!("expected seq {expected_seq}; a row is missing or reordered"),
            });
        }
        if row.prev_hash != head.hash {
            return Err(StoreError::ChainBroken {
                seq: row.seq,
                reason: "prev_hash does not link to the previous row".to_string(),
            });
        }
        let recomputed = row_hash(&row.prev_hash, row.seq, &row.kind, &row.body);
        if recomputed != row.hash {
            return Err(StoreError::ChainBroken {
                seq: row.seq,
                reason: "row hash does not match its content".to_string(),
            });
        }
        head = ChainHead {
            seq: row.seq,
            hash: row.hash,
        };
    }
    Ok(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(n: u64) -> Vec<StoredEvent> {
        let mut head = ChainHead::genesis();
        let mut rows = Vec::new();
        for i in 0..n {
            let row = next_row(&head, "start", format!("{{\"i\":{i}}}"));
            head = ChainHead {
                seq: row.seq,
                hash: row.hash,
            };
            rows.push(row);
        }
        rows
    }

    #[test]
    fn a_clean_chain_verifies() {
        let rows = chain(5);
        let head = verify(&rows).unwrap();
        assert_eq!(head.seq, 5);
        assert_eq!(head.hash, rows[4].hash);
        assert_eq!(verify(&[]).unwrap(), ChainHead::genesis());
    }

    #[test]
    fn edits_deletions_and_reorders_break_it() {
        let mut edited = chain(4);
        edited[1].body.push(' ');
        assert!(matches!(
            verify(&edited),
            Err(StoreError::ChainBroken { seq: 2, .. })
        ));

        let mut deleted = chain(4);
        deleted.remove(1);
        assert!(matches!(
            verify(&deleted),
            Err(StoreError::ChainBroken { seq: 3, .. })
        ));

        let mut reordered = chain(4);
        reordered.swap(1, 2);
        assert!(verify(&reordered).is_err());

        let mut relabeled = chain(3);
        relabeled[2].kind = "refund".into();
        assert!(verify(&relabeled).is_err());
    }

    #[test]
    fn length_prefixes_prevent_boundary_shifts() {
        assert_ne!(
            row_hash(&GENESIS_HASH, 1, "ab", "c"),
            row_hash(&GENESIS_HASH, 1, "a", "bc")
        );
    }
}
