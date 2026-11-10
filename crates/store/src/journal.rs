//! The storage trait and the in-memory backend.

use std::sync::{Arc, Mutex};

use crate::error::StoreError;

/// One stored journal row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub seq: u64,
    pub kind: String,
    pub body: String,
    pub prev_hash: [u8; 32],
    pub hash: [u8; 32],
}

/// One stored projection snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRow {
    /// The journal sequence number the snapshot reflects.
    pub seq: u64,
    /// The chain hash of row `seq`; binds the snapshot to the journal.
    pub chain_hash: [u8; 32],
    /// The serialized projection.
    pub image: String,
    /// SHA-256 of `image`.
    pub image_sha256: [u8; 32],
}

/// A backend for the append-only journal and its snapshots.
///
/// Backends store and return rows; they do not interpret them. Chain
/// verification, replay and the snapshot policy live in
/// [`crate::PersistentLedger`], so every backend gets the same checks.
pub trait LedgerStore {
    /// Durably append one row. The row's `seq` must be exactly one past
    /// the last stored row; anything else is an error and writes nothing.
    fn append(&mut self, row: &StoredEvent) -> Result<(), StoreError>;

    /// Every stored row, ordered by `seq`.
    fn load_events(&self) -> Result<Vec<StoredEvent>, StoreError>;

    /// Store a projection snapshot.
    fn put_snapshot(&mut self, snapshot: &SnapshotRow) -> Result<(), StoreError>;

    /// The snapshot with the highest `seq`, if any.
    fn latest_snapshot(&self) -> Result<Option<SnapshotRow>, StoreError>;
}

impl<S: LedgerStore + ?Sized> LedgerStore for Box<S> {
    fn append(&mut self, row: &StoredEvent) -> Result<(), StoreError> {
        (**self).append(row)
    }
    fn load_events(&self) -> Result<Vec<StoredEvent>, StoreError> {
        (**self).load_events()
    }
    fn put_snapshot(&mut self, snapshot: &SnapshotRow) -> Result<(), StoreError> {
        (**self).put_snapshot(snapshot)
    }
    fn latest_snapshot(&self) -> Result<Option<SnapshotRow>, StoreError> {
        (**self).latest_snapshot()
    }
}

#[derive(Debug, Default)]
struct MemoryInner {
    events: Vec<StoredEvent>,
    snapshots: Vec<SnapshotRow>,
}

/// In-memory backend, for tests and for the API's no-persistence mode.
///
/// Clones share the same rows, so a test can drop a ledger handle and
/// reopen a new one over the same journal, as after a process restart.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    inner: Arc<Mutex<MemoryInner>>,
}

impl MemoryStore {
    /// An empty journal.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, MemoryInner>, StoreError> {
        self.inner
            .lock()
            .map_err(|_| StoreError::Backend("memory store lock poisoned".to_string()))
    }

    /// Replace a stored row in place. Test-only tampering hook: the real
    /// journal API has no update path.
    pub fn tamper_row(&self, seq: u64, body: &str) -> Result<(), StoreError> {
        let mut inner = self.lock()?;
        let row = inner
            .events
            .iter_mut()
            .find(|row| row.seq == seq)
            .ok_or_else(|| StoreError::Validation(format!("no row {seq}")))?;
        row.body = body.to_string();
        Ok(())
    }

    /// Number of stored rows.
    pub fn len(&self) -> usize {
        self.lock().map(|inner| inner.events.len()).unwrap_or(0)
    }

    /// True when no row is stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl LedgerStore for MemoryStore {
    fn append(&mut self, row: &StoredEvent) -> Result<(), StoreError> {
        let mut inner = self.lock()?;
        let expected = inner.events.last().map(|r| r.seq).unwrap_or(0) + 1;
        if row.seq != expected {
            return Err(StoreError::Backend(format!(
                "append out of order: got seq {}, expected {expected}",
                row.seq
            )));
        }
        inner.events.push(row.clone());
        Ok(())
    }

    fn load_events(&self) -> Result<Vec<StoredEvent>, StoreError> {
        Ok(self.lock()?.events.clone())
    }

    fn put_snapshot(&mut self, snapshot: &SnapshotRow) -> Result<(), StoreError> {
        let mut inner = self.lock()?;
        inner.snapshots.retain(|s| s.seq != snapshot.seq);
        inner.snapshots.push(snapshot.clone());
        inner.snapshots.sort_by_key(|s| s.seq);
        Ok(())
    }

    fn latest_snapshot(&self) -> Result<Option<SnapshotRow>, StoreError> {
        Ok(self.lock()?.snapshots.last().cloned())
    }
}
