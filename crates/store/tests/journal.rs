//! Durability tests for the journaled ledger: replay equivalence, crash
//! windows, tamper detection, idempotency across reopen, and snapshots.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use aps_core::{Amount, ReportHash};
use aps_store::{
    CallerTable, IdempotencyOutcome, IntentState, LedgerStore, MemoryStore, OpenOptions,
    PersistentLedger, SnapshotRow, SqliteStore, StoreError, StoredEvent,
};

// ---------------------------------------------------------------- helpers

/// xorshift64: a std-only deterministic PRNG.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

/// A scratch database path, removed on drop.
struct TempDb(PathBuf);

impl TempDb {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("aps-store-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDb(dir)
    }
    fn path(&self) -> PathBuf {
        self.0.join("ledger.sqlite")
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn opts(every: u64) -> OpenOptions {
    OpenOptions {
        snapshot_every: every,
    }
}

fn hash_from(rng: &mut Rng) -> ReportHash {
    let mut bytes = [0u8; 32];
    for b in bytes.iter_mut() {
        *b = (rng.next() >> 24) as u8;
    }
    bytes[0] |= 1;
    ReportHash(bytes)
}

const ACCOUNTS: [&str; 3] = ["acct-a", "acct-b", "acct-c"];
const CALLERS: [&str; 2] = ["caller-1", "caller-2"];

/// Drive a random operation mix. Errors from the core are expected and
/// journal nothing. Returns every wire job id created.
fn drive<S: LedgerStore>(ledger: &mut PersistentLedger<S>, seed: u64, steps: usize) -> Vec<String> {
    let mut rng = Rng::new(seed);
    let mut jobs: Vec<String> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    let mut intents = 0u64;
    for step in 0..steps {
        let caller = CALLERS[rng.below(2) as usize];
        let account = ACCOUNTS[rng.below(3) as usize];
        match rng.below(9) {
            0 | 1 => {
                // Fresh key most of the time, a reused key sometimes: a
                // replay or a conflict, neither of which journals.
                let key = if !keys.is_empty() && rng.below(4) == 0 {
                    keys[rng.below(keys.len() as u64) as usize].clone()
                } else {
                    format!("dep-{seed}-{step}")
                };
                keys.push(key.clone());
                let amount = Amount::from_atomic(u128::from(1 + rng.below(5_000_000)));
                let _ = ledger.deposit(caller, account, amount, &key);
            }
            2 | 3 => {
                let amount = Amount::from_atomic(u128::from(1 + rng.below(2_000_000)));
                let key = format!("res-{seed}-{step}");
                if let Ok(r) = ledger.reserve(caller, account, "scan.v1", amount, &key) {
                    jobs.push(r.job_id.to_string());
                }
            }
            4 if !jobs.is_empty() => {
                let job = jobs[rng.below(jobs.len() as u64) as usize].clone();
                let _ = ledger.start(&job);
            }
            5 if !jobs.is_empty() => {
                let job = jobs[rng.below(jobs.len() as u64) as usize].clone();
                let _ = ledger.settle(&job, hash_from(&mut rng));
            }
            6 if !jobs.is_empty() => {
                let job = jobs[rng.below(jobs.len() as u64) as usize].clone();
                let _ = ledger.refund(&job, "fuzz refund");
            }
            7 => {
                intents += 1;
                let id = format!("intent-{}", rng.below(intents));
                let next = [
                    IntentState::Pending,
                    IntentState::Confirmed,
                    IntentState::Refunded,
                    IntentState::Expired,
                ][rng.below(4) as usize];
                let _ = ledger.record_intent_state(&id, account, next, Some("tx-ref"));
            }
            _ => {
                let key = format!("idem-{}", rng.below(40));
                let req = aps_store::to_hex(&aps_store::sha256(key.as_bytes()));
                let _ = ledger.record_idempotency(caller, "payment.intents", &key, &req, "{}");
            }
        }
        assert!(ledger.is_conserved(), "conservation after step {step}");
    }
    jobs
}

/// Assert two ledgers expose identical state.
fn assert_same_state<A: LedgerStore, B: LedgerStore>(
    a: &PersistentLedger<A>,
    b: &PersistentLedger<B>,
    jobs: &[String],
) {
    let (la, lb) = (a.ledger().unwrap(), b.ledger().unwrap());
    for account in ACCOUNTS {
        assert_eq!(
            la.balances(account).ok(),
            lb.balances(account).ok(),
            "{account}"
        );
    }
    assert_eq!(la.job_count(), lb.job_count());
    for job in jobs {
        let (ia, ib) = (a.job_id(job).unwrap(), b.job_id(job).unwrap());
        assert_eq!(la.job_snapshot(&ia).unwrap(), lb.job_snapshot(&ib).unwrap());
    }
    assert_eq!(a.projection().unwrap(), b.projection().unwrap());
    assert_eq!(a.head(), b.head());
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    FailBeforeWrite,
    CrashAfterWrite,
}

/// Wraps a store and injects a failure on the next append.
struct FaultStore<S> {
    inner: S,
    fault: Arc<Mutex<Fault>>,
}

impl<S: LedgerStore> LedgerStore for FaultStore<S> {
    fn append(&mut self, row: &StoredEvent) -> Result<(), StoreError> {
        let fault = *self.fault.lock().unwrap();
        match fault {
            Fault::None => self.inner.append(row),
            Fault::FailBeforeWrite => {
                Err(StoreError::Backend("simulated failure before write".into()))
            }
            Fault::CrashAfterWrite => {
                self.inner.append(row)?;
                Err(StoreError::Backend("simulated crash after commit".into()))
            }
        }
    }
    fn load_events(&self) -> Result<Vec<StoredEvent>, StoreError> {
        self.inner.load_events()
    }
    fn put_snapshot(&mut self, snapshot: &SnapshotRow) -> Result<(), StoreError> {
        self.inner.put_snapshot(snapshot)
    }
    fn latest_snapshot(&self) -> Result<Option<SnapshotRow>, StoreError> {
        self.inner.latest_snapshot()
    }
}

// ------------------------------------------------------- replay equivalence

#[test]
fn memory_replay_equivalence_over_random_sequences() {
    for seed in 1..=12 {
        let store = MemoryStore::new();
        let mut live = PersistentLedger::open(store.clone(), opts(0)).unwrap();
        let jobs = drive(&mut live, seed, 400);
        let reopened = PersistentLedger::open(store.clone(), opts(0)).unwrap();
        assert_same_state(&live, &reopened, &jobs);
        assert_eq!(reopened.open_report().events_replayed, store.len() as u64);
    }
}

#[test]
fn sqlite_replay_equivalence_over_random_sequences() {
    for seed in [3u64, 17, 99] {
        let db = TempDb::new(&format!("equiv-{seed}"));
        let mut live =
            PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(50)).unwrap();
        let jobs = drive(&mut live, seed, 300);
        let reopened =
            PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(50)).unwrap();
        assert_same_state(&live, &reopened, &jobs);
        drop(live);
        drop(reopened);
    }
}

#[test]
fn replays_and_failures_write_nothing() {
    let store = MemoryStore::new();
    let mut ledger = PersistentLedger::open(store.clone(), opts(0)).unwrap();
    ledger
        .deposit("c", "a", Amount::from_atomic(10), "k")
        .unwrap();
    let replay = ledger
        .deposit("c", "a", Amount::from_atomic(10), "k")
        .unwrap();
    assert!(replay.replayed);
    assert!(ledger
        .deposit("c", "a", Amount::from_atomic(11), "k")
        .is_err());
    assert!(ledger
        .reserve("c", "a", "scan.v1", Amount::from_atomic(11), "r")
        .is_err());
    let r = ledger
        .reserve("c", "a", "scan.v1", Amount::from_atomic(4), "r")
        .unwrap();
    let job = r.job_id.to_string();
    ledger.start(&job).unwrap();
    ledger.start(&job).unwrap();
    assert_eq!(
        store.len(),
        3,
        "deposit, reserve, start; the rest journal nothing"
    );
}

// ------------------------------------------------------------ crash windows

#[test]
fn crash_after_commit_before_ack_replays_on_retry() {
    let db = TempDb::new("crash-after");
    let fault = Arc::new(Mutex::new(Fault::None));
    {
        let store = FaultStore {
            inner: SqliteStore::open(db.path()).unwrap(),
            fault: fault.clone(),
        };
        let mut ledger = PersistentLedger::open(store, opts(0)).unwrap();
        ledger
            .deposit("c", "a", Amount::from_atomic(5_000_000), "d1")
            .unwrap();
        *fault.lock().unwrap() = Fault::CrashAfterWrite;
        let err = ledger
            .reserve("c", "a", "scan.v1", Amount::from_atomic(1_000_000), "r1")
            .unwrap_err();
        assert!(matches!(err, StoreError::Backend(_)));
        assert!(ledger.is_poisoned());
        assert_eq!(ledger.ledger().unwrap_err(), StoreError::Poisoned);
        assert_eq!(
            ledger
                .deposit("c", "a", Amount::from_atomic(1), "d2")
                .unwrap_err(),
            StoreError::Poisoned
        );
        // The process "dies" here: the client never saw an acknowledgement.
    }
    let mut ledger =
        PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(0)).unwrap();
    let balances = ledger.ledger().unwrap().balances("a").unwrap();
    assert_eq!(
        balances.reserved.atomic(),
        1_000_000,
        "the committed event survived"
    );
    // The client retries with the same key and gets the recorded receipt.
    let retry = ledger
        .reserve("c", "a", "scan.v1", Amount::from_atomic(1_000_000), "r1")
        .unwrap();
    assert!(retry.replayed);
    assert_eq!(retry.job_id.as_str(), "job_0000000000000001");
    assert_eq!(ledger.ledger().unwrap().balances("a").unwrap(), balances);
}

#[test]
fn failure_before_commit_applies_once_on_retry() {
    let db = TempDb::new("crash-before");
    let fault = Arc::new(Mutex::new(Fault::None));
    {
        let store = FaultStore {
            inner: SqliteStore::open(db.path()).unwrap(),
            fault: fault.clone(),
        };
        let mut ledger = PersistentLedger::open(store, opts(0)).unwrap();
        ledger
            .deposit("c", "a", Amount::from_atomic(5_000_000), "d1")
            .unwrap();
        *fault.lock().unwrap() = Fault::FailBeforeWrite;
        assert!(ledger
            .reserve("c", "a", "scan.v1", Amount::from_atomic(1_000_000), "r1")
            .is_err());
        assert!(ledger.is_poisoned());
    }
    let mut ledger =
        PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(0)).unwrap();
    assert_eq!(
        ledger.ledger().unwrap().job_count(),
        0,
        "nothing was journaled"
    );
    let retry = ledger
        .reserve("c", "a", "scan.v1", Amount::from_atomic(1_000_000), "r1")
        .unwrap();
    assert!(!retry.replayed);
    let b = ledger.ledger().unwrap().balances("a").unwrap();
    assert_eq!(
        (b.available.atomic(), b.reserved.atomic()),
        (4_000_000, 1_000_000)
    );
}

// --------------------------------------------------------------- tampering

fn seeded_db(db: &TempDb, every: u64) {
    let mut ledger =
        PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(every)).unwrap();
    ledger
        .deposit("c", "a", Amount::from_atomic(5000), "d1")
        .unwrap();
    let r = ledger
        .reserve("c", "a", "scan.v1", Amount::from_atomic(1000), "r1")
        .unwrap();
    let job = r.job_id.to_string();
    ledger.start(&job).unwrap();
    ledger.settle(&job, ReportHash([7; 32])).unwrap();
    ledger
        .deposit("c", "b", Amount::from_atomic(300), "d2")
        .unwrap();
    ledger
        .deposit("c", "b", Amount::from_atomic(300), "d3")
        .unwrap();
}

#[test]
fn sql_updates_and_deletes_are_refused_by_triggers() {
    let db = TempDb::new("triggers");
    seeded_db(&db, 0);
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    assert!(conn
        .execute("UPDATE events SET body = 'x' WHERE seq = 1", [])
        .is_err());
    assert!(conn
        .execute("DELETE FROM events WHERE seq = 6", [])
        .is_err());
}

#[test]
fn a_tampered_row_fails_to_open() {
    let db = TempDb::new("tamper");
    seeded_db(&db, 0);
    {
        let conn = rusqlite::Connection::open(db.path()).unwrap();
        conn.execute_batch("DROP TRIGGER events_no_update;")
            .unwrap();
        let changed = conn
            .execute(
                "UPDATE events SET body = replace(body, '\"5000\"', '\"9000\"') WHERE seq = 1",
                [],
            )
            .unwrap();
        assert_eq!(changed, 1);
    }
    let err = PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(0))
        .err()
        .expect("must refuse to open");
    assert!(
        matches!(err, StoreError::ChainBroken { seq: 1, .. }),
        "{err}"
    );
}

#[test]
fn a_deleted_interior_row_fails_to_open() {
    let db = TempDb::new("delete");
    seeded_db(&db, 0);
    {
        let conn = rusqlite::Connection::open(db.path()).unwrap();
        conn.execute_batch("DROP TRIGGER events_no_delete; DELETE FROM events WHERE seq = 3;")
            .unwrap();
    }
    let err = PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(0))
        .err()
        .expect("must refuse to open");
    assert!(
        matches!(err, StoreError::ChainBroken { seq: 4, .. }),
        "{err}"
    );
}

#[test]
fn truncation_behind_a_snapshot_fails_to_open() {
    let db = TempDb::new("truncate");
    seeded_db(&db, 6); // snapshot at seq 6, the last row
    {
        let conn = rusqlite::Connection::open(db.path()).unwrap();
        conn.execute_batch("DROP TRIGGER events_no_delete; DELETE FROM events WHERE seq >= 5;")
            .unwrap();
    }
    let err = PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(6))
        .err()
        .expect("must refuse to open");
    assert!(
        matches!(err, StoreError::SnapshotMismatch { seq: 6, .. }),
        "{err}"
    );
}

#[test]
fn a_consistent_but_wrong_outcome_is_a_replay_divergence() {
    // A forged journal whose chain verifies but whose reserve claims a job
    // id the core would never assign is refused at replay.
    let store = MemoryStore::new();
    let mut ledger = PersistentLedger::open(store.clone(), opts(0)).unwrap();
    ledger
        .deposit("c", "a", Amount::from_atomic(10), "d")
        .unwrap();
    ledger
        .reserve("c", "a", "scan.v1", Amount::from_atomic(3), "r")
        .unwrap();
    drop(ledger);
    let mut rows = store.load_events().unwrap();
    rows[1].body = rows[1]
        .body
        .replace("job_0000000000000001", "job_00000000000000ff");
    let mut forged = MemoryStore::new();
    let mut head = aps_store::ChainHead::genesis();
    for row in rows {
        let next = aps_store::chain::next_row(&head, &row.kind, row.body);
        head = aps_store::ChainHead {
            seq: next.seq,
            hash: next.hash,
        };
        forged.append(&next).unwrap();
    }
    let err = PersistentLedger::open(forged, opts(0))
        .err()
        .expect("refused");
    assert!(
        matches!(err, StoreError::ReplayDivergence { seq: 2, .. }),
        "{err}"
    );
}

#[test]
fn memory_tamper_hook_breaks_the_chain() {
    let store = MemoryStore::new();
    let mut ledger = PersistentLedger::open(store.clone(), opts(0)).unwrap();
    ledger
        .deposit("c", "a", Amount::from_atomic(10), "d")
        .unwrap();
    drop(ledger);
    store
        .tamper_row(
            1,
            r#"{"type":"deposit","caller":"c","account":"a","amount":"99","idempotency_key":"d"}"#,
        )
        .unwrap();
    assert!(matches!(
        PersistentLedger::open(store, opts(0)),
        Err(StoreError::ChainBroken { seq: 1, .. })
    ));
}

// ------------------------------------------------------------- idempotency

#[test]
fn an_idempotency_record_survives_reopen() {
    let db = TempDb::new("idem");
    let req = aps_store::to_hex(&aps_store::sha256(b"POST /payment/intents {...}"));
    let other = aps_store::to_hex(&aps_store::sha256(b"a different body"));
    {
        let mut ledger =
            PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(0)).unwrap();
        let outcome = ledger
            .record_idempotency(
                "agent-1",
                "payment.intents",
                "key-1",
                &req,
                r#"{"intent_id":"i-1"}"#,
            )
            .unwrap();
        assert_eq!(outcome, IdempotencyOutcome::Recorded);
        ledger
            .deposit("agent-1", "a", Amount::from_atomic(10), "dep-1")
            .unwrap();
    }
    let mut ledger =
        PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(0)).unwrap();
    let entry = ledger
        .idempotency("agent-1", "payment.intents", "key-1")
        .unwrap()
        .clone();
    assert_eq!(entry.response, r#"{"intent_id":"i-1"}"#);
    assert!(matches!(
        ledger.record_idempotency("agent-1", "payment.intents", "key-1", &req, "{}"),
        Ok(IdempotencyOutcome::Replayed(e)) if e == entry
    ));
    assert!(matches!(
        ledger.record_idempotency("agent-1", "payment.intents", "key-1", &other, "{}"),
        Err(StoreError::IdempotencyConflict(_))
    ));
    // Caller scoping: the same key for another caller is independent.
    assert_eq!(
        ledger
            .record_idempotency("agent-2", "payment.intents", "key-1", &other, "{}")
            .unwrap(),
        IdempotencyOutcome::Recorded
    );
    // Core-level idempotency also survives: the deposit replays.
    let replay = ledger
        .deposit("agent-1", "a", Amount::from_atomic(10), "dep-1")
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        ledger
            .ledger()
            .unwrap()
            .balances("a")
            .unwrap()
            .available
            .atomic(),
        10
    );
}

#[test]
fn intent_states_persist_and_illegal_moves_are_refused() {
    let store = MemoryStore::new();
    let mut ledger = PersistentLedger::open(store.clone(), opts(0)).unwrap();
    assert!(ledger
        .record_intent_state("i-1", "a", IntentState::Pending, None)
        .unwrap());
    assert!(!ledger
        .record_intent_state("i-1", "a", IntentState::Pending, None)
        .unwrap());
    assert!(ledger
        .record_intent_state("i-1", "a", IntentState::Confirmed, Some("tx-1"))
        .unwrap());
    assert!(matches!(
        ledger.record_intent_state("i-1", "a", IntentState::Pending, None),
        Err(StoreError::IllegalIntentTransition { .. })
    ));
    assert!(ledger
        .record_intent_state("i-1", "b", IntentState::Refunded, None)
        .is_err());
    drop(ledger);
    let ledger = PersistentLedger::open(store, opts(0)).unwrap();
    let intent = &ledger.projection().unwrap().intents["i-1"];
    assert_eq!(intent.state, IntentState::Confirmed);
    assert_eq!(intent.detail.as_deref(), Some("tx-1"));
}

// --------------------------------------------------------------- snapshots

#[test]
fn snapshot_plus_tail_equals_full_replay() {
    for seed in [5u64, 23, 41] {
        let db = TempDb::new(&format!("snap-{seed}"));
        let jobs = {
            let mut ledger =
                PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(7)).unwrap();
            drive(&mut ledger, seed, 250)
        };
        let store = SqliteStore::open(db.path()).unwrap();
        let full = PersistentLedger::full_replay_projection(&store).unwrap();
        let reopened = PersistentLedger::open(store, opts(7)).unwrap();
        let report = reopened.open_report();
        let snap = report.snapshot_seq.expect("a snapshot was written");
        assert!(snap > 0 && snap.is_multiple_of(7));
        assert_eq!(report.tail_applied, report.head.seq - snap);
        assert!(report.tail_applied < report.events_replayed);
        assert_eq!(reopened.projection().unwrap(), &full);
        // The same journal opened from a memory copy without snapshots
        // gives identical state.
        let mut copy = MemoryStore::new();
        for row in reopened.store().load_events().unwrap() {
            copy.append(&row).unwrap();
        }
        let plain = PersistentLedger::open(copy, opts(0)).unwrap();
        assert_eq!(plain.open_report().snapshot_seq, None);
        assert_same_state(&reopened, &plain, &jobs);
    }
}

#[test]
fn a_tampered_snapshot_fails_to_open() {
    let db = TempDb::new("snap-tamper");
    seeded_db(&db, 3);
    {
        let conn = rusqlite::Connection::open(db.path()).unwrap();
        conn.execute(
            "UPDATE snapshots SET image = replace(image, '\"5000\"', '\"5001\"')",
            [],
        )
        .unwrap();
    }
    let err = PersistentLedger::open(SqliteStore::open(db.path()).unwrap(), opts(3))
        .err()
        .expect("must refuse to open");
    assert!(matches!(err, StoreError::SnapshotMismatch { .. }), "{err}");
}

// ---------------------------------------------------- callers and nonces

#[test]
fn sqlite_callers_persist_by_digest_only() {
    let db = TempDb::new("callers");
    {
        let mut store = SqliteStore::open(db.path()).unwrap();
        store.insert_caller("agent-1", [9; 32], 100).unwrap();
        assert!(store.insert_caller("agent-1", [8; 32], 100).is_err());
        assert!(store.insert_caller("agent-2", [9; 32], 100).is_err());
        assert!(store.insert_caller("Bad Id", [7; 32], 100).is_err());
    }
    let mut store = SqliteStore::open(db.path()).unwrap();
    let found = store.find_by_key_hash(&[9; 32]).unwrap().unwrap();
    assert_eq!(found.caller_id, "agent-1");
    assert!(found.is_active());
    assert!(store.replace_key("agent-1", [6; 32]).unwrap());
    assert!(store.find_by_key_hash(&[9; 32]).unwrap().is_none());
    assert!(store.revoke_caller("agent-1", 200).unwrap());
    assert_eq!(store.list_callers().unwrap()[0].revoked_at, Some(200));
}

#[test]
fn sqlite_nonces_are_single_use_until_expiry() {
    let store = SqliteStore::open_in_memory().unwrap();
    assert!(store
        .consume_nonce("alice.testnet", &[1; 32], 1_000, 0)
        .unwrap());
    assert!(!store
        .consume_nonce("alice.testnet", &[1; 32], 1_000, 10)
        .unwrap());
    assert!(store
        .consume_nonce("bob.testnet", &[1; 32], 1_000, 10)
        .unwrap());
    // After expiry the row is pruned; the timestamp window rejects such a
    // stale request before the nonce table is consulted.
    assert!(store
        .consume_nonce("alice.testnet", &[1; 32], 3_000, 2_000)
        .unwrap());
}

#[test]
fn backup_copies_a_verifiable_journal() {
    let db = TempDb::new("backup");
    seeded_db(&db, 0);
    let out = db.0.join("backup.sqlite");
    SqliteStore::open(db.path())
        .unwrap()
        .backup_to(&out)
        .unwrap();
    let restored = PersistentLedger::open(SqliteStore::open(&out).unwrap(), opts(0)).unwrap();
    assert_eq!(restored.head().seq, 6);
    assert!(restored.is_conserved());
}
