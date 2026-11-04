//! The store's own serializable view of the ledger state.
//!
//! The projection applies recorded outcomes, not requests: it never
//! decides anything the core did not already decide. It exists so the
//! state can be snapshotted (the core `Ledger` has no export), and as an
//! independent second computation that must agree with the replayed core
//! ledger on every open.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use aps_core::{Balances, JobId, JobState, Ledger};

use crate::error::StoreError;
use crate::event::{atomic, LedgerEvent};

/// Balances of one account, in atomic units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountImage {
    #[serde(with = "atomic")]
    pub available: u128,
    #[serde(with = "atomic")]
    pub reserved: u128,
    #[serde(with = "atomic")]
    pub settled_total: u128,
    #[serde(with = "atomic")]
    pub refunded_total: u128,
    #[serde(with = "atomic")]
    pub deposited_total: u128,
}

impl AccountImage {
    /// The conservation invariant: deposited equals available plus
    /// reserved plus settled.
    pub fn is_conserved(&self) -> bool {
        self.available
            .checked_add(self.reserved)
            .and_then(|sum| sum.checked_add(self.settled_total))
            .map(|active| active == self.deposited_total)
            .unwrap_or(false)
    }

    fn matches(&self, core: &Balances) -> bool {
        self.available == core.available.atomic()
            && self.reserved == core.reserved.atomic()
            && self.settled_total == core.settled_total.atomic()
            && self.refunded_total == core.refunded_total.atomic()
            && self.deposited_total == core.deposited_total.atomic()
    }
}

/// One job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobImage {
    pub account: String,
    pub sku: String,
    #[serde(with = "atomic")]
    pub amount: u128,
    /// `reserved`, `running`, `settled` or `refunded`.
    pub state: String,
    pub report_hash: Option<String>,
    pub refund_reason: Option<String>,
}

/// Gateway-level state of a payment intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentState {
    Pending,
    Confirmed,
    Expired,
    Rejected,
    Refunded,
}

impl IntentState {
    /// Parse the wire label.
    pub fn parse(label: &str) -> Option<Self> {
        match label {
            "pending" => Some(IntentState::Pending),
            "confirmed" => Some(IntentState::Confirmed),
            "expired" => Some(IntentState::Expired),
            "rejected" => Some(IntentState::Rejected),
            "refunded" => Some(IntentState::Refunded),
            _ => None,
        }
    }

    /// The wire label.
    pub fn label(self) -> &'static str {
        match self {
            IntentState::Pending => "pending",
            IntentState::Confirmed => "confirmed",
            IntentState::Expired => "expired",
            IntentState::Rejected => "rejected",
            IntentState::Refunded => "refunded",
        }
    }

    /// Legal transitions: pending to confirmed, expired or rejected;
    /// confirmed to refunded. Everything else is terminal.
    pub fn can_move_to(self, next: IntentState) -> bool {
        matches!(
            (self, next),
            (IntentState::Pending, IntentState::Confirmed)
                | (IntentState::Pending, IntentState::Expired)
                | (IntentState::Pending, IntentState::Rejected)
                | (IntentState::Confirmed, IntentState::Refunded)
        )
    }
}

/// One payment intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentImage {
    pub account: String,
    pub state: IntentState,
    pub detail: Option<String>,
}

/// One API-level idempotency record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdempotencyEntry {
    pub request_sha256: String,
    pub response: String,
}

/// The full projected state at journal position `seq`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projection {
    pub seq: u64,
    pub accounts: BTreeMap<String, AccountImage>,
    pub jobs: BTreeMap<String, JobImage>,
    pub intents: BTreeMap<String, IntentImage>,
    /// API-level records, keyed by `caller US scope US key` (US = 0x1f).
    pub idempotency: BTreeMap<String, IdempotencyEntry>,
    /// Core-level idempotency keys already journaled (`caller US key`).
    /// The journal never holds a replay, so a repeat is corruption.
    pub core_keys: BTreeMap<String, String>,
}

/// Join key parts with the ASCII unit separator, which identifiers never
/// contain in practice and which cannot be confused with a delimiter.
pub(crate) fn joined(parts: &[&str]) -> String {
    parts.join("\u{1f}")
}

impl Projection {
    /// Apply one recorded event at `seq`.
    pub fn apply(&mut self, seq: u64, event: &LedgerEvent) -> Result<(), StoreError> {
        let fail = |reason: String| StoreError::ReplayDivergence { seq, reason };
        match event {
            LedgerEvent::Deposit {
                caller,
                account,
                amount,
                idempotency_key,
            } => {
                self.claim_core_key(caller, idempotency_key, "deposit")
                    .map_err(fail)?;
                if *amount == 0 {
                    return Err(fail("zero deposit".to_string()));
                }
                let image = self.accounts.entry(account.clone()).or_default();
                image.deposited_total = add(image.deposited_total, *amount).map_err(fail)?;
                image.available = add(image.available, *amount).map_err(fail)?;
            }
            LedgerEvent::Reserve {
                caller,
                account,
                sku,
                amount,
                idempotency_key,
                job_id,
            } => {
                self.claim_core_key(caller, idempotency_key, "reserve")
                    .map_err(fail)?;
                if self.jobs.contains_key(job_id) {
                    return Err(fail(format!("job {job_id} reserved twice")));
                }
                let image = self
                    .accounts
                    .get_mut(account)
                    .ok_or_else(|| fail(format!("reserve on unknown account {account}")))?;
                image.available = sub(image.available, *amount).map_err(fail)?;
                image.reserved = add(image.reserved, *amount).map_err(fail)?;
                self.jobs.insert(
                    job_id.clone(),
                    JobImage {
                        account: account.clone(),
                        sku: sku.clone(),
                        amount: *amount,
                        state: "reserved".to_string(),
                        report_hash: None,
                        refund_reason: None,
                    },
                );
            }
            LedgerEvent::Start { job_id } => {
                let job = self
                    .jobs
                    .get_mut(job_id)
                    .ok_or_else(|| fail(format!("start on unknown job {job_id}")))?;
                if job.state != "reserved" {
                    return Err(fail(format!("start from {}", job.state)));
                }
                job.state = "running".to_string();
            }
            LedgerEvent::Settle {
                job_id,
                report_hash,
            } => {
                let job = self
                    .jobs
                    .get_mut(job_id)
                    .ok_or_else(|| fail(format!("settle on unknown job {job_id}")))?;
                if job.state != "running" {
                    return Err(fail(format!("settle from {}", job.state)));
                }
                job.state = "settled".to_string();
                job.report_hash = Some(report_hash.clone());
                let (account, amount) = (job.account.clone(), job.amount);
                let image = self
                    .accounts
                    .get_mut(&account)
                    .ok_or_else(|| fail(format!("job account {account} unknown")))?;
                image.reserved = sub(image.reserved, amount).map_err(fail)?;
                image.settled_total = add(image.settled_total, amount).map_err(fail)?;
            }
            LedgerEvent::Refund { job_id, reason } => {
                let job = self
                    .jobs
                    .get_mut(job_id)
                    .ok_or_else(|| fail(format!("refund on unknown job {job_id}")))?;
                if job.state != "reserved" && job.state != "running" {
                    return Err(fail(format!("refund from {}", job.state)));
                }
                job.state = "refunded".to_string();
                job.refund_reason = Some(reason.clone());
                let (account, amount) = (job.account.clone(), job.amount);
                let image = self
                    .accounts
                    .get_mut(&account)
                    .ok_or_else(|| fail(format!("job account {account} unknown")))?;
                image.reserved = sub(image.reserved, amount).map_err(fail)?;
                image.available = add(image.available, amount).map_err(fail)?;
                image.refunded_total = add(image.refunded_total, amount).map_err(fail)?;
            }
            LedgerEvent::IntentState {
                intent_id,
                account,
                state,
                detail,
            } => {
                let next = IntentState::parse(state)
                    .ok_or_else(|| fail(format!("unknown intent state {state:?}")))?;
                if let Some(current) = self.intents.get(intent_id) {
                    if current.account != *account {
                        return Err(fail(format!("intent {intent_id} changed account")));
                    }
                    if !current.state.can_move_to(next) {
                        return Err(fail(format!(
                            "intent {intent_id}: {} -> {}",
                            current.state.label(),
                            next.label()
                        )));
                    }
                }
                self.intents.insert(
                    intent_id.clone(),
                    IntentImage {
                        account: account.clone(),
                        state: next,
                        detail: detail.clone(),
                    },
                );
            }
            LedgerEvent::IdempotencyRecord {
                caller,
                scope,
                key,
                request_sha256,
                response,
            } => {
                let joined_key = joined(&[caller, scope, key]);
                if self.idempotency.contains_key(&joined_key) {
                    return Err(fail(format!("idempotency record {scope}/{key} repeated")));
                }
                self.idempotency.insert(
                    joined_key,
                    IdempotencyEntry {
                        request_sha256: request_sha256.clone(),
                        response: response.clone(),
                    },
                );
            }
        }
        self.seq = seq;
        Ok(())
    }

    fn claim_core_key(&mut self, caller: &str, key: &str, kind: &str) -> Result<(), String> {
        let joined_key = joined(&[caller, key]);
        if self.core_keys.contains_key(&joined_key) {
            return Err(format!("core idempotency key {key:?} journaled twice"));
        }
        self.core_keys.insert(joined_key, kind.to_string());
        Ok(())
    }

    /// Every account satisfies conservation.
    pub fn check_conservation(&self) -> Result<(), StoreError> {
        for (account, image) in &self.accounts {
            if !image.is_conserved() {
                return Err(StoreError::ConservationViolation(format!(
                    "account {account}"
                )));
            }
        }
        Ok(())
    }

    /// Cross-check this projection against a replayed core ledger.
    pub fn check_against(
        &self,
        ledger: &Ledger,
        job_ids: &BTreeMap<String, JobId>,
    ) -> Result<(), StoreError> {
        let mismatch = StoreError::ProjectionMismatch;
        if !ledger.is_conserved() {
            return Err(StoreError::ConservationViolation(
                "the core ledger reports a violation".to_string(),
            ));
        }
        self.check_conservation()?;
        for (account, image) in &self.accounts {
            let core = ledger
                .balances(account)
                .map_err(|_| mismatch(format!("account {account} missing in core")))?;
            if !image.matches(&core) {
                return Err(mismatch(format!("balances of {account}")));
            }
        }
        if ledger.job_count() != self.jobs.len() || job_ids.len() != self.jobs.len() {
            return Err(mismatch(format!(
                "job count: core {}, projection {}",
                ledger.job_count(),
                self.jobs.len()
            )));
        }
        for (wire, image) in &self.jobs {
            let id = job_ids
                .get(wire)
                .ok_or_else(|| mismatch(format!("job {wire} has no core id")))?;
            let snap = ledger
                .job_snapshot(id)
                .map_err(|_| mismatch(format!("job {wire} missing in core")))?;
            let core_hash = snap.settlement.as_ref().map(|s| s.report_hash.to_hex());
            let core_reason = snap.refund.as_ref().map(|r| r.reason.clone());
            if snap.account != image.account
                || snap.sku != image.sku
                || snap.amount.atomic() != image.amount
                || state_label(snap.state) != image.state
                || core_hash != image.report_hash
                || core_reason != image.refund_reason
            {
                return Err(mismatch(format!("job {wire}")));
            }
        }
        Ok(())
    }

    /// Serialize for a snapshot.
    pub fn to_image(&self) -> Result<String, StoreError> {
        serde_json::to_string(self)
            .map_err(|err| StoreError::Validation(format!("cannot encode projection: {err}")))
    }

    /// Decode a snapshot image.
    pub fn from_image(seq: u64, image: &str) -> Result<Self, StoreError> {
        serde_json::from_str(image).map_err(|err| StoreError::SnapshotMismatch {
            seq,
            reason: format!("undecodable image: {err}"),
        })
    }

    /// Sum of all balances across accounts: (deposited, available,
    /// reserved, settled, refunded). Display helper for the ledger page.
    pub fn totals(&self) -> AccountImage {
        let mut total = AccountImage::default();
        for image in self.accounts.values() {
            total.available = total.available.saturating_add(image.available);
            total.reserved = total.reserved.saturating_add(image.reserved);
            total.settled_total = total.settled_total.saturating_add(image.settled_total);
            total.refunded_total = total.refunded_total.saturating_add(image.refunded_total);
            total.deposited_total = total.deposited_total.saturating_add(image.deposited_total);
        }
        total
    }
}

fn state_label(state: JobState) -> String {
    state.to_string()
}

fn add(a: u128, b: u128) -> Result<u128, String> {
    a.checked_add(b).ok_or_else(|| "overflow".to_string())
}

fn sub(a: u128, b: u128) -> Result<u128, String> {
    a.checked_sub(b).ok_or_else(|| "underflow".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_transitions_are_one_way() {
        use IntentState::*;
        assert!(Pending.can_move_to(Confirmed));
        assert!(Confirmed.can_move_to(Refunded));
        assert!(!Confirmed.can_move_to(Pending));
        assert!(!Expired.can_move_to(Confirmed));
        assert!(!Refunded.can_move_to(Refunded));
        for state in [Pending, Confirmed, Expired, Rejected, Refunded] {
            assert_eq!(IntentState::parse(state.label()), Some(state));
        }
    }

    #[test]
    fn reserve_beyond_available_is_divergence() {
        let mut p = Projection::default();
        p.apply(
            1,
            &LedgerEvent::Deposit {
                caller: "c".into(),
                account: "a".into(),
                amount: 5,
                idempotency_key: "k1".into(),
            },
        )
        .unwrap();
        let err = p
            .apply(
                2,
                &LedgerEvent::Reserve {
                    caller: "c".into(),
                    account: "a".into(),
                    sku: "scan.v1".into(),
                    amount: 6,
                    idempotency_key: "k2".into(),
                    job_id: "job_1".into(),
                },
            )
            .unwrap_err();
        assert!(matches!(err, StoreError::ReplayDivergence { seq: 2, .. }));
    }
}
