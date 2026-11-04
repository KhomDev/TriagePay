//! The durable ledger handle: the core ledger, its journal, and the
//! projection, kept in lockstep.

use std::collections::BTreeMap;

use aps_core::{
    Amount, DepositReceipt, JobId, JobState, Ledger, RefundReceipt, ReportHash, Reservation,
    Settlement,
};

use crate::chain::{self, ChainHead};
use crate::error::StoreError;
use crate::event::LedgerEvent;
use crate::journal::{LedgerStore, SnapshotRow, StoredEvent};
use crate::projection::{joined, IdempotencyEntry, IntentState, Projection};

/// Open-time settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenOptions {
    /// Write a projection snapshot every N journaled events; 0 disables.
    pub snapshot_every: u64,
}

impl Default for OpenOptions {
    fn default() -> Self {
        OpenOptions {
            snapshot_every: 1000,
        }
    }
}

/// What happened while opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenReport {
    /// Events replayed into the core ledger (always the whole journal).
    pub events_replayed: u64,
    /// The snapshot the projection started from, if any.
    pub snapshot_seq: Option<u64>,
    /// Events applied to the projection after the snapshot.
    pub tail_applied: u64,
    /// The verified chain head.
    pub head: ChainHead,
}

/// Result of recording an API-level idempotency entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyOutcome {
    /// The entry is new and now journaled.
    Recorded,
    /// The same request was recorded before; this is its stored entry.
    Replayed(IdempotencyEntry),
}

/// A core ledger whose every mutation is journaled.
pub struct PersistentLedger<S: LedgerStore> {
    store: S,
    ledger: Ledger,
    projection: Projection,
    job_ids: BTreeMap<String, JobId>,
    head: ChainHead,
    options: OpenOptions,
    poisoned: bool,
    snapshot_failures: u64,
    report: OpenReport,
}

impl<S: LedgerStore> PersistentLedger<S> {
    /// Open a journal: verify the chain, replay it into a fresh core
    /// ledger, restore the projection from the newest snapshot plus the
    /// tail, and cross-check the two. Any failure refuses to open.
    pub fn open(store: S, options: OpenOptions) -> Result<Self, StoreError> {
        let rows = store.load_events()?;
        let head = chain::verify(&rows)?;
        let events = decode_all(&rows)?;

        let mut ledger = Ledger::new();
        let mut job_ids = BTreeMap::new();
        for (seq, event) in &events {
            replay_core(&mut ledger, &mut job_ids, *seq, event)?;
        }

        let (mut projection, snapshot_seq) = match store.latest_snapshot()? {
            None => (Projection::default(), None),
            Some(snapshot) => (verify_snapshot(&snapshot, &rows)?, Some(snapshot.seq)),
        };
        let mut tail_applied = 0;
        for (seq, event) in &events {
            if *seq > projection.seq {
                projection.apply(*seq, event)?;
                tail_applied += 1;
            }
        }
        projection.check_against(&ledger, &job_ids)?;

        let report = OpenReport {
            events_replayed: events.len() as u64,
            snapshot_seq,
            tail_applied,
            head,
        };
        Ok(PersistentLedger {
            store,
            ledger,
            projection,
            job_ids,
            head,
            options,
            poisoned: false,
            snapshot_failures: 0,
            report,
        })
    }

    /// Rebuild the projection from the journal alone, ignoring snapshots.
    /// Used by operators and tests to prove snapshot plus tail equals a
    /// full replay.
    pub fn full_replay_projection(store: &S) -> Result<Projection, StoreError> {
        let rows = store.load_events()?;
        chain::verify(&rows)?;
        let mut projection = Projection::default();
        for (seq, event) in decode_all(&rows)? {
            projection.apply(seq, &event)?;
        }
        Ok(projection)
    }

    fn usable(&self) -> Result<(), StoreError> {
        if self.poisoned {
            Err(StoreError::Poisoned)
        } else {
            Ok(())
        }
    }

    /// Journal one event after the in-memory state already changed.
    fn commit(&mut self, event: LedgerEvent) -> Result<(), StoreError> {
        let body = match event.to_body() {
            Ok(body) => body,
            Err(err) => {
                self.poisoned = true;
                return Err(err);
            }
        };
        let row = chain::next_row(&self.head, event.kind(), body);
        if let Err(err) = self.store.append(&row) {
            self.poisoned = true;
            return Err(err);
        }
        self.head = ChainHead {
            seq: row.seq,
            hash: row.hash,
        };
        if let Err(err) = self.projection.apply(row.seq, &event) {
            self.poisoned = true;
            return Err(StoreError::ProjectionMismatch(err.to_string()));
        }
        if self.options.snapshot_every > 0
            && row.seq.is_multiple_of(self.options.snapshot_every)
            && self.snapshot_now().is_err()
        {
            // A missed snapshot costs replay time, never correctness.
            self.snapshot_failures += 1;
        }
        Ok(())
    }

    /// Write a projection snapshot at the current head.
    pub fn snapshot_now(&mut self) -> Result<(), StoreError> {
        self.usable()?;
        if self.head.seq == 0 {
            return Ok(());
        }
        let image = self.projection.to_image()?;
        let snapshot = SnapshotRow {
            seq: self.head.seq,
            chain_hash: self.head.hash,
            image_sha256: crate::sha256(image.as_bytes()),
            image,
        };
        self.store.put_snapshot(&snapshot)
    }

    /// Deposit credit. Journaled unless it is an idempotent replay.
    pub fn deposit(
        &mut self,
        caller: &str,
        account: &str,
        amount: Amount,
        idempotency_key: &str,
    ) -> Result<DepositReceipt, StoreError> {
        self.usable()?;
        let receipt = self
            .ledger
            .deposit(caller, account, amount, idempotency_key)?;
        if !receipt.replayed {
            self.commit(LedgerEvent::Deposit {
                caller: caller.to_string(),
                account: account.to_string(),
                amount: amount.atomic(),
                idempotency_key: idempotency_key.to_string(),
            })?;
        }
        Ok(receipt)
    }

    /// Reserve credit for a job. Journaled unless it is a replay.
    pub fn reserve(
        &mut self,
        caller: &str,
        account: &str,
        sku: &str,
        amount: Amount,
        idempotency_key: &str,
    ) -> Result<Reservation, StoreError> {
        self.usable()?;
        let reservation = self
            .ledger
            .reserve(caller, account, sku, amount, idempotency_key)?;
        if !reservation.replayed {
            self.job_ids
                .insert(reservation.job_id.to_string(), reservation.job_id.clone());
            self.commit(LedgerEvent::Reserve {
                caller: caller.to_string(),
                account: account.to_string(),
                sku: sku.to_string(),
                amount: amount.atomic(),
                idempotency_key: idempotency_key.to_string(),
                job_id: reservation.job_id.to_string(),
            })?;
        }
        Ok(reservation)
    }

    /// Resolve a wire job id to the core id, `UnknownJob` when absent.
    pub fn job_id(&self, wire: &str) -> Result<JobId, StoreError> {
        self.job_ids
            .get(wire)
            .cloned()
            .ok_or_else(|| StoreError::Core(aps_core::CoreError::UnknownJob(wire.to_string())))
    }

    /// Move a job to running. Only the real transition is journaled.
    pub fn start(&mut self, job_id: &str) -> Result<JobState, StoreError> {
        self.usable()?;
        let id = self.job_id(job_id)?;
        let before = self.ledger.job_state(&id)?;
        let state = self.ledger.start(&id)?;
        if before == JobState::Reserved {
            self.commit(LedgerEvent::Start {
                job_id: job_id.to_string(),
            })?;
        }
        Ok(state)
    }

    /// Settle a running job. Journaled unless it is a replay.
    pub fn settle(
        &mut self,
        job_id: &str,
        report_hash: ReportHash,
    ) -> Result<Settlement, StoreError> {
        self.usable()?;
        let id = self.job_id(job_id)?;
        let settlement = self.ledger.settle(&id, report_hash)?;
        if !settlement.replayed {
            self.commit(LedgerEvent::Settle {
                job_id: job_id.to_string(),
                report_hash: report_hash.to_hex(),
            })?;
        }
        Ok(settlement)
    }

    /// Refund a reserved or running job. Journaled unless it is a replay.
    pub fn refund(&mut self, job_id: &str, reason: &str) -> Result<RefundReceipt, StoreError> {
        self.usable()?;
        let id = self.job_id(job_id)?;
        let receipt = self.ledger.refund(&id, reason)?;
        if !receipt.replayed {
            self.commit(LedgerEvent::Refund {
                job_id: job_id.to_string(),
                reason: reason.to_string(),
            })?;
        }
        Ok(receipt)
    }

    /// Record a payment-intent state change. Returns false, journaling
    /// nothing, when the intent is already in that state.
    pub fn record_intent_state(
        &mut self,
        intent_id: &str,
        account: &str,
        state: IntentState,
        detail: Option<&str>,
    ) -> Result<bool, StoreError> {
        self.usable()?;
        if intent_id.trim().is_empty() || account.trim().is_empty() {
            return Err(StoreError::Validation(
                "intent id and account must be non-empty".to_string(),
            ));
        }
        if let Some(current) = self.projection.intents.get(intent_id) {
            if current.account != account {
                return Err(StoreError::Validation(format!(
                    "intent {intent_id} belongs to another account"
                )));
            }
            if current.state == state {
                return Ok(false);
            }
            if !current.state.can_move_to(state) {
                return Err(StoreError::IllegalIntentTransition {
                    intent: intent_id.to_string(),
                    from: current.state.label().to_string(),
                    to: state.label().to_string(),
                });
            }
        }
        self.commit(LedgerEvent::IntentState {
            intent_id: intent_id.to_string(),
            account: account.to_string(),
            state: state.label().to_string(),
            detail: detail.map(str::to_string),
        })?;
        Ok(true)
    }

    /// Record the response to an API request under a caller-scoped
    /// idempotency key. The same request replays the stored entry; a
    /// different request under the same key is a conflict.
    pub fn record_idempotency(
        &mut self,
        caller: &str,
        scope: &str,
        key: &str,
        request_sha256: &str,
        response: &str,
    ) -> Result<IdempotencyOutcome, StoreError> {
        self.usable()?;
        if crate::from_hex32(request_sha256).is_none() {
            return Err(StoreError::Validation(
                "request_sha256 must be 64 hex characters".to_string(),
            ));
        }
        if key.is_empty() || scope.is_empty() {
            return Err(StoreError::Validation(
                "idempotency scope and key must be non-empty".to_string(),
            ));
        }
        if let Some(entry) = self.idempotency(caller, scope, key) {
            if entry.request_sha256.eq_ignore_ascii_case(request_sha256) {
                return Ok(IdempotencyOutcome::Replayed(entry.clone()));
            }
            return Err(StoreError::IdempotencyConflict(key.to_string()));
        }
        self.commit(LedgerEvent::IdempotencyRecord {
            caller: caller.to_string(),
            scope: scope.to_string(),
            key: key.to_string(),
            request_sha256: request_sha256.to_ascii_lowercase(),
            response: response.to_string(),
        })?;
        Ok(IdempotencyOutcome::Recorded)
    }

    /// Look up an API-level idempotency entry.
    pub fn idempotency(&self, caller: &str, scope: &str, key: &str) -> Option<&IdempotencyEntry> {
        self.projection
            .idempotency
            .get(&joined(&[caller, scope, key]))
    }

    /// The core ledger, for reads. Refused while poisoned, because the
    /// in-memory state may then hold a mutation the journal lacks.
    pub fn ledger(&self) -> Result<&Ledger, StoreError> {
        self.usable()?;
        Ok(&self.ledger)
    }

    /// The projection, for reads. Refused while poisoned.
    pub fn projection(&self) -> Result<&Projection, StoreError> {
        self.usable()?;
        Ok(&self.projection)
    }

    /// The current chain head.
    pub fn head(&self) -> ChainHead {
        self.head
    }

    /// True after a failed journal write.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Snapshots that failed to write since open.
    pub fn snapshot_failures(&self) -> u64 {
        self.snapshot_failures
    }

    /// What the open did.
    pub fn open_report(&self) -> OpenReport {
        self.report
    }

    /// The backend, for maintenance reads.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Conservation over both the core ledger and the projection.
    pub fn is_conserved(&self) -> bool {
        self.ledger.is_conserved() && self.projection.check_conservation().is_ok()
    }
}

fn decode_all(rows: &[StoredEvent]) -> Result<Vec<(u64, LedgerEvent)>, StoreError> {
    rows.iter()
        .map(|row| LedgerEvent::from_body(row.seq, &row.kind, &row.body).map(|e| (row.seq, e)))
        .collect()
}

fn verify_snapshot(snapshot: &SnapshotRow, rows: &[StoredEvent]) -> Result<Projection, StoreError> {
    let fail = |reason: &str| StoreError::SnapshotMismatch {
        seq: snapshot.seq,
        reason: reason.to_string(),
    };
    if snapshot.seq == 0 {
        return Err(fail("snapshot at seq 0"));
    }
    let row = rows
        .get((snapshot.seq - 1) as usize)
        .ok_or_else(|| fail("snapshot lies beyond the journal end; the journal was truncated"))?;
    if row.hash != snapshot.chain_hash {
        return Err(fail("chain hash differs from the journal row"));
    }
    if crate::sha256(snapshot.image.as_bytes()) != snapshot.image_sha256 {
        return Err(fail("image digest mismatch"));
    }
    let projection = Projection::from_image(snapshot.seq, &snapshot.image)?;
    if projection.seq != snapshot.seq {
        return Err(fail("image seq differs from the snapshot seq"));
    }
    Ok(projection)
}

/// Re-execute one journaled event through the core and require the same
/// outcome that was recorded.
fn replay_core(
    ledger: &mut Ledger,
    job_ids: &mut BTreeMap<String, JobId>,
    seq: u64,
    event: &LedgerEvent,
) -> Result<(), StoreError> {
    let fail = |reason: String| StoreError::ReplayDivergence { seq, reason };
    let lookup = |job_ids: &BTreeMap<String, JobId>, wire: &str| {
        job_ids
            .get(wire)
            .cloned()
            .ok_or_else(|| fail(format!("unknown job {wire}")))
    };
    match event {
        LedgerEvent::Deposit {
            caller,
            account,
            amount,
            idempotency_key,
        } => {
            let receipt = ledger
                .deposit(
                    caller,
                    account,
                    Amount::from_atomic(*amount),
                    idempotency_key,
                )
                .map_err(|err| fail(err.to_string()))?;
            if receipt.replayed {
                return Err(fail("deposit replayed instead of applying".to_string()));
            }
        }
        LedgerEvent::Reserve {
            caller,
            account,
            sku,
            amount,
            idempotency_key,
            job_id,
        } => {
            let reservation = ledger
                .reserve(
                    caller,
                    account,
                    sku,
                    Amount::from_atomic(*amount),
                    idempotency_key,
                )
                .map_err(|err| fail(err.to_string()))?;
            if reservation.replayed || reservation.job_id.as_str() != job_id {
                return Err(fail(format!(
                    "reserve produced job {} (replayed {}), journal says {job_id}",
                    reservation.job_id, reservation.replayed
                )));
            }
            job_ids.insert(job_id.clone(), reservation.job_id);
        }
        LedgerEvent::Start { job_id } => {
            let id = lookup(job_ids, job_id)?;
            let before = ledger.job_state(&id).map_err(|err| fail(err.to_string()))?;
            if before != JobState::Reserved {
                return Err(fail(format!("start from {before}")));
            }
            ledger.start(&id).map_err(|err| fail(err.to_string()))?;
        }
        LedgerEvent::Settle {
            job_id,
            report_hash,
        } => {
            let id = lookup(job_ids, job_id)?;
            let hash = ReportHash::from_hex(report_hash).map_err(|err| fail(err.to_string()))?;
            let settlement = ledger
                .settle(&id, hash)
                .map_err(|err| fail(err.to_string()))?;
            if settlement.replayed {
                return Err(fail("settle replayed instead of applying".to_string()));
            }
        }
        LedgerEvent::Refund { job_id, reason } => {
            let id = lookup(job_ids, job_id)?;
            let receipt = ledger
                .refund(&id, reason)
                .map_err(|err| fail(err.to_string()))?;
            if receipt.replayed {
                return Err(fail("refund replayed instead of applying".to_string()));
            }
        }
        LedgerEvent::IntentState { .. } | LedgerEvent::IdempotencyRecord { .. } => {}
    }
    Ok(())
}
