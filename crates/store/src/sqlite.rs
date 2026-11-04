//! SQLite backend: one database file holds the journal, the snapshots,
//! the caller registry and the NEP-413 nonce table.
//!
//! The `events` table is append-only at the database level: triggers
//! abort every UPDATE and DELETE. That stops accidental writes through
//! SQL; it does not stop someone who can drop the triggers, which is what
//! the hash chain is for.

use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};

use crate::callers::{validate_caller_id, CallerRecord, CallerTable};
use crate::error::StoreError;
use crate::journal::{LedgerStore, SnapshotRow, StoredEvent};

/// On-disk schema version.
pub const SCHEMA_VERSION: &str = "1";

/// Snapshots kept after each write (older ones are pruned).
const SNAPSHOTS_KEPT: i64 = 3;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
    seq       INTEGER PRIMARY KEY,
    kind      TEXT NOT NULL,
    body      TEXT NOT NULL,
    prev_hash BLOB NOT NULL,
    hash      BLOB NOT NULL
);
CREATE TRIGGER IF NOT EXISTS events_no_update BEFORE UPDATE ON events
BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;
CREATE TRIGGER IF NOT EXISTS events_no_delete BEFORE DELETE ON events
BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;
CREATE TABLE IF NOT EXISTS snapshots (
    seq          INTEGER PRIMARY KEY,
    chain_hash   BLOB NOT NULL,
    image        TEXT NOT NULL,
    image_sha256 BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS callers (
    caller_id  TEXT PRIMARY KEY,
    key_sha256 BLOB NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    revoked_at INTEGER
);
CREATE TABLE IF NOT EXISTS nonces (
    account_id    TEXT NOT NULL,
    nonce         BLOB NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    PRIMARY KEY (account_id, nonce)
);
";

/// A SQLite-backed store.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Open or create the database file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        Self::init(conn, true)
    }

    /// A private in-memory database, for tests.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn, false)
    }

    fn init(conn: Connection, on_disk: bool) -> Result<Self, StoreError> {
        conn.busy_timeout(Duration::from_secs(5))?;
        if on_disk {
            let _mode: String =
                conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        }
        // FULL: a committed append survives power loss, not just a crash.
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION],
        )?;
        let version: String = conn.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )?;
        if version != SCHEMA_VERSION {
            return Err(StoreError::Backend(format!(
                "unsupported schema version {version}, expected {SCHEMA_VERSION}"
            )));
        }
        Ok(SqliteStore { conn })
    }

    /// Write a consistent copy of the whole database to `out` (which must
    /// not exist). Safe while the service runs.
    pub fn backup_to(&self, out: impl AsRef<Path>) -> Result<(), StoreError> {
        let target = out
            .as_ref()
            .to_str()
            .ok_or_else(|| StoreError::Validation("backup path must be UTF-8".to_string()))?;
        self.conn.execute("VACUUM INTO ?1", params![target])?;
        Ok(())
    }

    /// Consume a NEP-413 nonce for an account. Returns true when the nonce
    /// is fresh (and records it until `expires_at_ms`), false on replay.
    /// Expired nonces are pruned first.
    pub fn consume_nonce(
        &self,
        account_id: &str,
        nonce: &[u8; 32],
        expires_at_ms: u64,
        now_ms: u64,
    ) -> Result<bool, StoreError> {
        self.conn.execute(
            "DELETE FROM nonces WHERE expires_at_ms < ?1",
            params![to_i64(now_ms)?],
        )?;
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO nonces (account_id, nonce, expires_at_ms) VALUES (?1, ?2, ?3)",
            params![account_id, &nonce[..], to_i64(expires_at_ms)?],
        )?;
        Ok(inserted == 1)
    }
}

fn to_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Validation("value exceeds i64".to_string()))
}

fn to_u64(value: i64, seq: u64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt {
        seq,
        reason: "negative integer column".to_string(),
    })
}

fn blob32(bytes: Vec<u8>, seq: u64, column: &str) -> Result<[u8; 32], StoreError> {
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| StoreError::Corrupt {
        seq,
        reason: format!("{column} is not 32 bytes"),
    })
}

impl LedgerStore for SqliteStore {
    fn append(&mut self, row: &StoredEvent) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let last: i64 =
            tx.query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?;
        let seq = to_i64(row.seq)?;
        if seq != last + 1 {
            return Err(StoreError::Backend(format!(
                "append out of order: got seq {seq}, expected {}",
                last + 1
            )));
        }
        tx.execute(
            "INSERT INTO events (seq, kind, body, prev_hash, hash) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![seq, row.kind, row.body, &row.prev_hash[..], &row.hash[..]],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn load_events(&self) -> Result<Vec<StoredEvent>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, kind, body, prev_hash, hash FROM events ORDER BY seq")?;
        let raw = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|(seq, kind, body, prev, hash)| {
                let seq = to_u64(seq, 0)?;
                Ok(StoredEvent {
                    seq,
                    kind,
                    body,
                    prev_hash: blob32(prev, seq, "prev_hash")?,
                    hash: blob32(hash, seq, "hash")?,
                })
            })
            .collect()
    }

    fn put_snapshot(&mut self, snapshot: &SnapshotRow) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO snapshots (seq, chain_hash, image, image_sha256)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                to_i64(snapshot.seq)?,
                &snapshot.chain_hash[..],
                snapshot.image,
                &snapshot.image_sha256[..]
            ],
        )?;
        tx.execute(
            "DELETE FROM snapshots WHERE seq NOT IN
             (SELECT seq FROM snapshots ORDER BY seq DESC LIMIT ?1)",
            params![SNAPSHOTS_KEPT],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn latest_snapshot(&self) -> Result<Option<SnapshotRow>, StoreError> {
        let raw = self
            .conn
            .query_row(
                "SELECT seq, chain_hash, image, image_sha256 FROM snapshots
                 ORDER BY seq DESC LIMIT 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .optional()?;
        match raw {
            None => Ok(None),
            Some((seq, chain_hash, image, digest)) => {
                let seq = to_u64(seq, 0)?;
                Ok(Some(SnapshotRow {
                    seq,
                    chain_hash: blob32(chain_hash, seq, "snapshot chain_hash")?,
                    image,
                    image_sha256: blob32(digest, seq, "snapshot image_sha256")?,
                }))
            }
        }
    }
}

fn caller_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, Vec<u8>, i64, Option<i64>)> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
}

fn caller_record(raw: (String, Vec<u8>, i64, Option<i64>)) -> Result<CallerRecord, StoreError> {
    let (caller_id, key, created, revoked) = raw;
    Ok(CallerRecord {
        caller_id,
        key_sha256: blob32(key, 0, "caller key_sha256")?,
        created_at: to_u64(created, 0)?,
        revoked_at: revoked.map(|v| to_u64(v, 0)).transpose()?,
    })
}

impl CallerTable for SqliteStore {
    fn insert_caller(
        &mut self,
        caller_id: &str,
        key_sha256: [u8; 32],
        now: u64,
    ) -> Result<(), StoreError> {
        validate_caller_id(caller_id)?;
        self.conn
            .execute(
                "INSERT INTO callers (caller_id, key_sha256, created_at) VALUES (?1, ?2, ?3)",
                params![caller_id, &key_sha256[..], to_i64(now)?],
            )
            .map_err(|err| match err {
                rusqlite::Error::SqliteFailure(code, _)
                    if code.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    StoreError::Validation(format!("caller {caller_id} or its key already exists"))
                }
                other => StoreError::from(other),
            })?;
        Ok(())
    }

    fn revoke_caller(&mut self, caller_id: &str, now: u64) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE callers SET revoked_at = ?2 WHERE caller_id = ?1 AND revoked_at IS NULL",
            params![caller_id, to_i64(now)?],
        )?;
        Ok(changed == 1)
    }

    fn replace_key(&mut self, caller_id: &str, key_sha256: [u8; 32]) -> Result<bool, StoreError> {
        let changed = self
            .conn
            .execute(
                "UPDATE callers SET key_sha256 = ?2 WHERE caller_id = ?1 AND revoked_at IS NULL",
                params![caller_id, &key_sha256[..]],
            )
            .map_err(|err| match err {
                rusqlite::Error::SqliteFailure(code, _)
                    if code.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    StoreError::Validation("key digest already in use".to_string())
                }
                other => StoreError::from(other),
            })?;
        Ok(changed == 1)
    }

    fn find_by_key_hash(&self, key_sha256: &[u8; 32]) -> Result<Option<CallerRecord>, StoreError> {
        let raw = self
            .conn
            .query_row(
                "SELECT caller_id, key_sha256, created_at, revoked_at FROM callers
                 WHERE key_sha256 = ?1",
                params![&key_sha256[..]],
                caller_from_row,
            )
            .optional()?;
        raw.map(caller_record).transpose()
    }

    fn list_callers(&self) -> Result<Vec<CallerRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT caller_id, key_sha256, created_at, revoked_at FROM callers ORDER BY caller_id",
        )?;
        let raw = stmt
            .query_map([], caller_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        raw.into_iter().map(caller_record).collect()
    }
}
