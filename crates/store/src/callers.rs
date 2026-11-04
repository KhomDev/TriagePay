//! The caller registry: API-key callers stored by key digest only.
//!
//! The registry never sees a key, only `SHA-256(key bytes)`. Callers are
//! not ledger mutations, so they live outside the hash-chained journal;
//! revocation is an in-place update.

use crate::error::StoreError;

/// One registered caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerRecord {
    pub caller_id: String,
    pub key_sha256: [u8; 32],
    /// Unix seconds.
    pub created_at: u64,
    /// Unix seconds; `None` while the caller is active.
    pub revoked_at: Option<u64>,
}

impl CallerRecord {
    /// True while the caller may authenticate.
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }
}

/// Validate a caller id: 1 to 64 characters of `a-z 0-9 . _ -`.
pub fn validate_caller_id(caller_id: &str) -> Result<(), StoreError> {
    let ok = !caller_id.is_empty()
        && caller_id.len() <= 64
        && caller_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(StoreError::Validation(
            "caller id must be 1-64 characters of a-z, 0-9, '.', '_' or '-'".to_string(),
        ))
    }
}

/// Storage for callers.
pub trait CallerTable {
    /// Register a new caller. Fails if the id or the key digest exists.
    fn insert_caller(
        &mut self,
        caller_id: &str,
        key_sha256: [u8; 32],
        now: u64,
    ) -> Result<(), StoreError>;

    /// Revoke a caller. Returns false when it was unknown or already
    /// revoked.
    fn revoke_caller(&mut self, caller_id: &str, now: u64) -> Result<bool, StoreError>;

    /// Replace an active caller's key digest (key rotation). Returns false
    /// when the caller is unknown or revoked.
    fn replace_key(&mut self, caller_id: &str, key_sha256: [u8; 32]) -> Result<bool, StoreError>;

    /// Find the caller holding a key digest, active or not.
    fn find_by_key_hash(&self, key_sha256: &[u8; 32]) -> Result<Option<CallerRecord>, StoreError>;

    /// Every caller, ordered by id.
    fn list_callers(&self) -> Result<Vec<CallerRecord>, StoreError>;
}

/// In-memory caller registry for tests and the no-persistence mode.
#[derive(Debug, Default, Clone)]
pub struct MemoryCallers {
    records: Vec<CallerRecord>,
}

impl MemoryCallers {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }
}

impl CallerTable for MemoryCallers {
    fn insert_caller(
        &mut self,
        caller_id: &str,
        key_sha256: [u8; 32],
        now: u64,
    ) -> Result<(), StoreError> {
        validate_caller_id(caller_id)?;
        if self
            .records
            .iter()
            .any(|r| r.caller_id == caller_id || r.key_sha256 == key_sha256)
        {
            return Err(StoreError::Validation(format!(
                "caller {caller_id} or its key already exists"
            )));
        }
        self.records.push(CallerRecord {
            caller_id: caller_id.to_string(),
            key_sha256,
            created_at: now,
            revoked_at: None,
        });
        self.records.sort_by(|a, b| a.caller_id.cmp(&b.caller_id));
        Ok(())
    }

    fn revoke_caller(&mut self, caller_id: &str, now: u64) -> Result<bool, StoreError> {
        match self
            .records
            .iter_mut()
            .find(|r| r.caller_id == caller_id && r.revoked_at.is_none())
        {
            Some(record) => {
                record.revoked_at = Some(now);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn replace_key(&mut self, caller_id: &str, key_sha256: [u8; 32]) -> Result<bool, StoreError> {
        if self.records.iter().any(|r| r.key_sha256 == key_sha256) {
            return Err(StoreError::Validation(
                "key digest already in use".to_string(),
            ));
        }
        match self
            .records
            .iter_mut()
            .find(|r| r.caller_id == caller_id && r.revoked_at.is_none())
        {
            Some(record) => {
                record.key_sha256 = key_sha256;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn find_by_key_hash(&self, key_sha256: &[u8; 32]) -> Result<Option<CallerRecord>, StoreError> {
        Ok(self
            .records
            .iter()
            .find(|r| r.key_sha256 == *key_sha256)
            .cloned())
    }

    fn list_callers(&self) -> Result<Vec<CallerRecord>, StoreError> {
        Ok(self.records.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_ids_are_validated() {
        for good in ["a", "agent-1", "x.y_z", &"a".repeat(64)] {
            assert!(validate_caller_id(good).is_ok(), "{good}");
        }
        for bad in ["", "A", "a b", "a/b", &"a".repeat(65), "é"] {
            assert!(validate_caller_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn memory_registry_revokes_and_rotates() {
        let mut table = MemoryCallers::new();
        table.insert_caller("agent-1", [1; 32], 10).unwrap();
        assert!(table.insert_caller("agent-1", [2; 32], 10).is_err());
        assert!(table.insert_caller("agent-2", [1; 32], 10).is_err());
        assert!(table.replace_key("agent-1", [3; 32]).unwrap());
        assert!(table.find_by_key_hash(&[1; 32]).unwrap().is_none());
        assert!(table.revoke_caller("agent-1", 20).unwrap());
        assert!(!table.revoke_caller("agent-1", 21).unwrap());
        let record = table.find_by_key_hash(&[3; 32]).unwrap().unwrap();
        assert!(!record.is_active());
        assert!(!table.replace_key("agent-1", [4; 32]).unwrap());
    }
}
