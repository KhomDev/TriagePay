//! The prepaid ledger: idempotent deposits, reservations, settlement, and
//! refunds over integer money, with structural conservation invariants.
//!
//! Single-writer by construction: the API crate wraps it in a lock at the
//! boundary. Every money-moving operation records an idempotency entry
//! keyed by caller plus client key; replays return the stored receipt and
//! never move funds twice.

use std::collections::HashMap;

use crate::account::Balances;
use crate::error::CoreError;
use crate::money::Amount;
use crate::state::{JobId, JobState, ReportHash};

/// Receipt for a successful deposit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositReceipt {
    pub account: String,
    pub credited: Amount,
    pub replayed: bool,
    pub balances: Balances,
}

/// Receipt for a reservation that locked credit for a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    pub job_id: JobId,
    pub account: String,
    pub sku: String,
    pub reserved: Amount,
    pub replayed: bool,
    pub balances: Balances,
}

/// Receipt for a settlement that consumed reserved credit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub job_id: JobId,
    pub charged: Amount,
    pub report_hash: ReportHash,
    pub replayed: bool,
    pub balances: Balances,
}

/// Receipt for a refund that returned reserved credit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundReceipt {
    pub job_id: JobId,
    pub refunded: Amount,
    pub reason: String,
    pub replayed: bool,
    pub balances: Balances,
}

/// Read-only view of a job exposed to the API layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSnapshot {
    pub job_id: JobId,
    pub account: String,
    pub sku: String,
    pub amount: Amount,
    pub state: JobState,
    pub settlement: Option<Settlement>,
    pub refund: Option<RefundReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum IdemRecord {
    Deposit(DepositReceipt),
    Reserve(Reservation),
}

#[derive(Debug, Clone)]
struct Job {
    id: JobId,
    account: String,
    sku: String,
    amount: Amount,
    state: JobState,
    settlement: Option<Settlement>,
    refund: Option<RefundReceipt>,
}

/// In-memory prepaid ledger.
#[derive(Debug, Default)]
pub struct Ledger {
    accounts: HashMap<String, Balances>,
    jobs: HashMap<JobId, Job>,
    idempotency: HashMap<String, (String, IdemRecord)>,
    next_job: u64,
}

impl Ledger {
    /// An empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    fn idem_key(caller: &str, key: &str) -> String {
        let mut k = String::with_capacity(caller.len() + key.len() + 1);
        k.push_str(caller);
        k.push(0x1f as char);
        k.push_str(key);
        k
    }

    /// Deposit credit, creating the account on first success. Idempotent
    /// per caller and key: the same payload replays the receipt; a changed
    /// payload under the same key is a conflict error.
    pub fn deposit(
        &mut self,
        caller: &str,
        account_id: &str,
        amount: Amount,
        idem_key: &str,
    ) -> Result<DepositReceipt, CoreError> {
        let payload = format!("deposit|{}|{}", account_id, amount.atomic());
        let key = Self::idem_key(caller, idem_key);
        if let Some((stored, record)) = self.idempotency.get(&key) {
            if *stored == payload {
                if let IdemRecord::Deposit(r) = record {
                    return Ok(DepositReceipt {
                        replayed: true,
                        ..r.clone()
                    });
                }
            }
            return Err(CoreError::IdempotencyConflict(idem_key.to_string()));
        }
        if amount.is_zero() {
            return Err(CoreError::Validation(
                "deposit amount must be positive".to_string(),
            ));
        }
        if account_id.trim().is_empty() {
            return Err(CoreError::Validation(
                "account id must be non-empty".to_string(),
            ));
        }
        let balances = self.accounts.entry(account_id.to_string()).or_default();
        balances.deposited_total = balances.deposited_total.checked_add(amount)?;
        balances.available = balances.available.checked_add(amount)?;
        let receipt = DepositReceipt {
            account: account_id.to_string(),
            credited: amount,
            replayed: false,
            balances: *balances,
        };
        self.idempotency
            .insert(key, (payload, IdemRecord::Deposit(receipt.clone())));
        Ok(receipt)
    }

    /// Reserve credit for a job. The account must already exist and hold
    /// sufficient available credit. Idempotent per caller and key.
    pub fn reserve(
        &mut self,
        caller: &str,
        account_id: &str,
        sku: &str,
        amount: Amount,
        idem_key: &str,
    ) -> Result<Reservation, CoreError> {
        let payload = format!("reserve|{}|{}|{}", account_id, sku, amount.atomic());
        let key = Self::idem_key(caller, idem_key);
        if let Some((stored, record)) = self.idempotency.get(&key) {
            if *stored == payload {
                if let IdemRecord::Reserve(r) = record {
                    return Ok(Reservation {
                        replayed: true,
                        ..r.clone()
                    });
                }
            }
            return Err(CoreError::IdempotencyConflict(idem_key.to_string()));
        }
        if amount.is_zero() {
            return Err(CoreError::Validation(
                "reservation amount must be positive".to_string(),
            ));
        }
        if sku.trim().is_empty() {
            return Err(CoreError::Validation("sku must be non-empty".to_string()));
        }
        {
            let balances = self
                .accounts
                .get_mut(account_id)
                .ok_or_else(|| CoreError::UnknownAccount(account_id.to_string()))?;
            if balances.available < amount {
                return Err(CoreError::InsufficientFunds {
                    account: account_id.to_string(),
                    needed: amount,
                    available: balances.available,
                });
            }
            balances.available = balances.available.checked_sub(amount)?;
            balances.reserved = balances.reserved.checked_add(amount)?;
        }
        self.next_job += 1;
        let id = JobId::new(format!("job_{:016x}", self.next_job));
        let job = Job {
            id: id.clone(),
            account: account_id.to_string(),
            sku: sku.to_string(),
            amount,
            state: JobState::Reserved,
            settlement: None,
            refund: None,
        };
        self.jobs.insert(id.clone(), job);
        let balances = *self
            .accounts
            .get(account_id)
            .ok_or_else(|| CoreError::UnknownAccount(account_id.to_string()))?;
        let reservation = Reservation {
            job_id: id,
            account: account_id.to_string(),
            sku: sku.to_string(),
            reserved: amount,
            replayed: false,
            balances,
        };
        self.idempotency
            .insert(key, (payload, IdemRecord::Reserve(reservation.clone())));
        Ok(reservation)
    }

    /// Move a job from Reserved to Running. Idempotent: starting a Running
    /// job again succeeds without effect.
    pub fn start(&mut self, job_id: &JobId) -> Result<JobState, CoreError> {
        let job = self
            .jobs
            .get_mut(job_id)
            .ok_or_else(|| CoreError::UnknownJob(job_id.to_string()))?;
        match job.state {
            JobState::Reserved => {
                job.state = JobState::Running;
                Ok(job.state)
            }
            JobState::Running => Ok(JobState::Running),
            other => Err(CoreError::IllegalTransition {
                job: job_id.to_string(),
                from: other.to_string(),
                to: "running".to_string(),
            }),
        }
    }

    /// Settle a Running job against a non-zero report hash, consuming the
    /// reserved credit. Settling again with the same hash replays the
    /// stored receipt; a different hash under the same job is a hard error.
    pub fn settle(
        &mut self,
        job_id: &JobId,
        report_hash: ReportHash,
    ) -> Result<Settlement, CoreError> {
        if report_hash.is_zero() {
            return Err(CoreError::Validation(
                "report hash must be non-zero".to_string(),
            ));
        }
        let (amount, account_id, state) = {
            let job = self
                .jobs
                .get(job_id)
                .ok_or_else(|| CoreError::UnknownJob(job_id.to_string()))?;
            (job.amount, job.account.clone(), job.state)
        };
        if state == JobState::Settled {
            let existing = {
                let job = self
                    .jobs
                    .get(job_id)
                    .ok_or_else(|| CoreError::UnknownJob(job_id.to_string()))?;
                job.settlement.clone()
            };
            if let Some(mut s) = existing {
                if s.report_hash == report_hash {
                    s.replayed = true;
                    return Ok(s);
                }
            }
            return Err(CoreError::IllegalTransition {
                job: job_id.to_string(),
                from: "settled".to_string(),
                to: "settled with a different report hash".to_string(),
            });
        }
        if state != JobState::Running {
            return Err(CoreError::IllegalTransition {
                job: job_id.to_string(),
                from: state.to_string(),
                to: "settled".to_string(),
            });
        }
        {
            let balances = self
                .accounts
                .get_mut(&account_id)
                .ok_or_else(|| CoreError::UnknownAccount(account_id.clone()))?;
            balances.reserved = balances.reserved.checked_sub(amount)?;
            balances.settled_total = balances.settled_total.checked_add(amount)?;
        }
        let balances = *self
            .accounts
            .get(&account_id)
            .ok_or_else(|| CoreError::UnknownAccount(account_id.clone()))?;
        let settlement = Settlement {
            job_id: job_id.clone(),
            charged: amount,
            report_hash,
            replayed: false,
            balances,
        };
        let job = self
            .jobs
            .get_mut(job_id)
            .ok_or_else(|| CoreError::UnknownJob(job_id.to_string()))?;
        job.state = JobState::Settled;
        job.settlement = Some(settlement.clone());
        Ok(settlement)
    }

    /// Refund a Reserved or Running job, returning its credit to available.
    /// Refunding a Refunded job replays the stored receipt; refunding a
    /// Settled job is a hard error.
    pub fn refund(&mut self, job_id: &JobId, reason: &str) -> Result<RefundReceipt, CoreError> {
        let (amount, account_id, state) = {
            let job = self
                .jobs
                .get(job_id)
                .ok_or_else(|| CoreError::UnknownJob(job_id.to_string()))?;
            (job.amount, job.account.clone(), job.state)
        };
        if state == JobState::Refunded {
            let existing = {
                let job = self
                    .jobs
                    .get(job_id)
                    .ok_or_else(|| CoreError::UnknownJob(job_id.to_string()))?;
                job.refund.clone()
            };
            if let Some(mut r) = existing {
                r.replayed = true;
                return Ok(r);
            }
            return Err(CoreError::IllegalTransition {
                job: job_id.to_string(),
                from: "refunded".to_string(),
                to: "refunded".to_string(),
            });
        }
        if state == JobState::Settled {
            return Err(CoreError::IllegalTransition {
                job: job_id.to_string(),
                from: "settled".to_string(),
                to: "refunded".to_string(),
            });
        }
        {
            let balances = self
                .accounts
                .get_mut(&account_id)
                .ok_or_else(|| CoreError::UnknownAccount(account_id.clone()))?;
            balances.reserved = balances.reserved.checked_sub(amount)?;
            balances.available = balances.available.checked_add(amount)?;
            balances.refunded_total = balances.refunded_total.checked_add(amount)?;
        }
        let balances = *self
            .accounts
            .get(&account_id)
            .ok_or_else(|| CoreError::UnknownAccount(account_id.clone()))?;
        let receipt = RefundReceipt {
            job_id: job_id.clone(),
            refunded: amount,
            reason: reason.to_string(),
            replayed: false,
            balances,
        };
        let job = self
            .jobs
            .get_mut(job_id)
            .ok_or_else(|| CoreError::UnknownJob(job_id.to_string()))?;
        job.state = JobState::Refunded;
        job.refund = Some(receipt.clone());
        Ok(receipt)
    }

    /// Current balances of an account.
    pub fn balances(&self, account_id: &str) -> Result<Balances, CoreError> {
        self.accounts
            .get(account_id)
            .copied()
            .ok_or_else(|| CoreError::UnknownAccount(account_id.to_string()))
    }

    /// Current state of a job.
    pub fn job_state(&self, job_id: &JobId) -> Result<JobState, CoreError> {
        self.jobs
            .get(job_id)
            .map(|j| j.state)
            .ok_or_else(|| CoreError::UnknownJob(job_id.to_string()))
    }

    /// Public snapshot of a job for API reads. Carries no history beyond
    /// the current state and terminal receipts.
    pub fn job_snapshot(&self, job_id: &JobId) -> Result<JobSnapshot, CoreError> {
        let job = self
            .jobs
            .get(job_id)
            .ok_or_else(|| CoreError::UnknownJob(job_id.to_string()))?;
        Ok(JobSnapshot {
            job_id: job.id.clone(),
            account: job.account.clone(),
            sku: job.sku.clone(),
            amount: job.amount,
            state: job.state,
            settlement: job.settlement.clone(),
            refund: job.refund.clone(),
        })
    }

    /// Number of jobs ever created.
    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }

    /// True when every account satisfies the conservation invariant.
    pub fn is_conserved(&self) -> bool {
        self.accounts.values().all(Balances::is_conserved)
    }
}
