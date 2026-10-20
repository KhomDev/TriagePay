//! Deep randomized conservation fuzz for the prepaid ledger.
//!
//! This goes beyond the sampling fuzz in `ledger_ops.rs`: conservation is
//! asserted after EVERY operation (not every 97 steps), a shadow model of
//! every balance is compared against the ledger after every operation,
//! multiple accounts are interleaved, idempotency replays and deliberate
//! same-key-changed-payload conflicts are driven against recorded keys,
//! refunds are proven one-shot, and settle is proven to bind exactly one
//! hash per job. Overflow edges pin the u128 ceiling as a typed error.
//!
//! The RNG is a std-only xorshift64 seeded per test: fully deterministic,
//! no external crates, no sleeps, no clock reads.

use std::collections::HashMap;

use aps_core::{Amount, Balances, CoreError, JobId, JobState, Ledger, ReportHash};

/// The single caller every fuzz operation acts under.
const CALLER: &str = "fuzz-caller";
/// The SKU every fuzz reservation buys.
const SKU: &str = "scan.v1";

/// xorshift64: a std-only deterministic PRNG.
struct XorShift64(u64);

impl XorShift64 {
    /// Any non-zero seed; `| 1` keeps the state odd and non-zero.
    fn new(seed: u64) -> Self {
        XorShift64(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A uniform-ish value in `0..bound`.
    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

/// A random non-zero 32-byte report hash. The first byte is forced odd so
/// the hash can never be the all-zero hash the ledger rejects.
fn random_hash(rng: &mut XorShift64) -> ReportHash {
    let mut bytes = [0u8; 32];
    for byte in bytes.iter_mut() {
        *byte = (rng.next_u64() >> 32) as u8;
    }
    bytes[0] |= 1;
    ReportHash(bytes)
}

/// A random positive amount of at most `max_atomic` atomic units.
fn random_amount(rng: &mut XorShift64, max_atomic: u64) -> Amount {
    Amount::from_atomic(u128::from(1 + rng.below(max_atomic)))
}

/// A previously recorded deposit, replayable by key.
#[derive(Clone)]
struct DepositRecord {
    key: String,
    account: String,
    amount: Amount,
}

/// A previously recorded reservation, replayable by key.
#[derive(Clone)]
struct ReserveRecord {
    key: String,
    account: String,
    sku: String,
    amount: Amount,
}

/// Coverage counters: the final assertions require every operation class
/// to have actually been exercised.
#[derive(Default)]
struct FuzzStats {
    deposits: u32,
    deposit_replays: u32,
    deposit_conflicts: u32,
    reserves: u32,
    reserve_replays: u32,
    reserve_conflicts: u32,
    insufficient: u32,
    starts: u32,
    start_rejections: u32,
    settles: u32,
    settle_replays: u32,
    settle_rejections: u32,
    refunds: u32,
    refund_replays: u32,
    refund_rejections: u32,
}

/// The fuzz driver: a real ledger plus an independent model of what every
/// account's balances must be.
struct FuzzState {
    ledger: Ledger,
    accounts: Vec<String>,
    /// Shadow model: account -> balances the ledger must report.
    expected: HashMap<String, Balances>,
    /// Every job ever created, in creation order.
    jobs: Vec<JobId>,
    /// Hash bound by each settled job.
    settled_hashes: HashMap<JobId, ReportHash>,
    /// Settled jobs in settlement order (deterministic iteration).
    settled_jobs: Vec<JobId>,
    deposit_records: Vec<DepositRecord>,
    reserve_records: Vec<ReserveRecord>,
    stats: FuzzStats,
}

impl FuzzState {
    /// Four accounts seeded with credit and one already-running job, so
    /// every operation class has a target from step zero.
    fn new(accounts: &[&str]) -> Self {
        let mut ledger = Ledger::new();
        let mut expected = HashMap::new();
        for (index, account) in accounts.iter().enumerate() {
            let receipt = ledger
                .deposit(
                    CALLER,
                    account,
                    Amount::from_atomic(3_000_000),
                    &format!("seed-d{index}"),
                )
                .expect("seed deposit succeeds");
            expected.insert(account.to_string(), receipt.balances);
            // The receipt is the shadow model's ground truth.
            assert_eq!(ledger.balances(account).unwrap(), receipt.balances);
        }
        assert!(ledger.is_conserved());

        // One pre-started job so start/settle/refund have live targets.
        let reservation = ledger
            .reserve(
                CALLER,
                accounts[0],
                SKU,
                Amount::from_atomic(1_000_000),
                "seed-r0",
            )
            .expect("seed reserve succeeds");
        let mut after = expected[accounts[0]];
        after.available = after
            .available
            .checked_sub(Amount::from_atomic(1_000_000))
            .expect("seed sub");
        after.reserved = after
            .reserved
            .checked_add(Amount::from_atomic(1_000_000))
            .expect("seed add");
        expected.insert(accounts[0].to_string(), after);
        ledger
            .start(&reservation.job_id)
            .expect("seed start succeeds");
        assert!(ledger.is_conserved());

        let state = FuzzState {
            ledger,
            accounts: accounts.iter().map(|a| a.to_string()).collect(),
            expected,
            jobs: vec![reservation.job_id.clone()],
            settled_hashes: HashMap::new(),
            settled_jobs: Vec::new(),
            deposit_records: Vec::new(),
            reserve_records: Vec::new(),
            stats: FuzzStats::default(),
        };
        state.verify(u32::MAX, "seed");
        state
    }

    /// The invariant checked after EVERY operation: conservation holds and
    /// the shadow model matches the ledger exactly.
    fn verify(&self, step: u32, what: &str) {
        assert!(
            self.ledger.is_conserved(),
            "conservation broke at step {step} after {what}"
        );
        for account in &self.accounts {
            let actual = self
                .ledger
                .balances(account)
                .expect("shadow account exists");
            let expected = self.expected[account];
            assert_eq!(
                actual, expected,
                "shadow model diverged for {account} at step {step} after {what}"
            );
        }
    }

    fn pick_account(&self, rng: &mut XorShift64) -> String {
        self.accounts[rng.below(self.accounts.len() as u64) as usize].clone()
    }

    fn pick_job(&self, rng: &mut XorShift64) -> Option<JobId> {
        if self.jobs.is_empty() {
            return None;
        }
        Some(self.jobs[rng.below(self.jobs.len() as u64) as usize].clone())
    }

    fn do_deposit(&mut self, rng: &mut XorShift64, step: u32) {
        let account = self.pick_account(rng);
        let amount = random_amount(rng, 2_500_000);
        let key = format!("d{step}");
        let before = self.expected[&account];
        let receipt = self
            .ledger
            .deposit(CALLER, &account, amount, &key)
            .expect("a fresh deposit within the fuzz budget succeeds");
        assert!(!receipt.replayed, "a fresh key never replays");
        assert_eq!(receipt.credited, amount);
        let mut after = before;
        after.deposited_total = before
            .deposited_total
            .checked_add(amount)
            .expect("shadow deposit add");
        after.available = before
            .available
            .checked_add(amount)
            .expect("shadow available add");
        self.expected.insert(account.clone(), after);
        self.deposit_records.push(DepositRecord {
            key,
            account,
            amount,
        });
        self.stats.deposits += 1;
        self.verify(step, "deposit");
    }

    fn do_deposit_replay(&mut self, rng: &mut XorShift64, step: u32) {
        let Some(record) = self.pick_deposit_record(rng) else {
            return;
        };
        let before = self.expected[&record.account];
        let receipt = self
            .ledger
            .deposit(CALLER, &record.account, record.amount, &record.key)
            .expect("an exact recorded replay succeeds");
        assert!(
            receipt.replayed,
            "a recorded key replays, not double-credits"
        );
        assert_eq!(receipt.credited, record.amount);
        assert_eq!(
            self.ledger.balances(&record.account).unwrap(),
            before,
            "a replay moves no money"
        );
        self.stats.deposit_replays += 1;
        self.verify(step, "deposit replay");
    }

    fn do_deposit_conflict(&mut self, rng: &mut XorShift64, step: u32) {
        let Some(record) = self.pick_deposit_record(rng) else {
            return;
        };
        let changed = record
            .amount
            .checked_add(Amount::from_atomic(1))
            .expect("conflict amount");
        let before = self.expected[&record.account];
        match self
            .ledger
            .deposit(CALLER, &record.account, changed, &record.key)
        {
            Err(CoreError::IdempotencyConflict(_)) => {}
            other => panic!("same key with a changed payload must conflict, got {other:?}"),
        }
        assert_eq!(
            self.ledger.balances(&record.account).unwrap(),
            before,
            "a conflicted deposit moves no money"
        );
        self.stats.deposit_conflicts += 1;
        self.verify(step, "deposit conflict");
    }

    fn pick_deposit_record(&self, rng: &mut XorShift64) -> Option<DepositRecord> {
        if self.deposit_records.is_empty() {
            return None;
        }
        let index = rng.below(self.deposit_records.len() as u64) as usize;
        Some(self.deposit_records[index].clone())
    }

    fn do_reserve(&mut self, rng: &mut XorShift64, step: u32) {
        let account = self.pick_account(rng);
        let amount = random_amount(rng, 2_000_000);
        let key = format!("r{step}");
        let before = self.expected[&account];
        let affordable = before.available >= amount;
        match self.ledger.reserve(CALLER, &account, SKU, amount, &key) {
            Ok(reservation) => {
                assert!(affordable, "an unaffordable reserve must not succeed");
                assert!(!reservation.replayed, "a fresh key never replays");
                let mut after = before;
                after.available = before
                    .available
                    .checked_sub(amount)
                    .expect("shadow reserve sub");
                after.reserved = before
                    .reserved
                    .checked_add(amount)
                    .expect("shadow reserve add");
                self.expected.insert(account.clone(), after);
                self.jobs.push(reservation.job_id.clone());
                self.reserve_records.push(ReserveRecord {
                    key,
                    account,
                    sku: SKU.to_string(),
                    amount,
                });
                self.stats.reserves += 1;
            }
            Err(CoreError::InsufficientFunds {
                account: denied,
                needed,
                available,
            }) => {
                assert!(!affordable, "an affordable reserve must not fail");
                assert_eq!(denied, account);
                assert_eq!(needed, amount);
                assert_eq!(available, before.available);
                self.stats.insufficient += 1;
            }
            other => panic!("unexpected reserve outcome: {other:?}"),
        }
        self.verify(step, "reserve");
    }

    fn do_reserve_replay(&mut self, rng: &mut XorShift64, step: u32) {
        let Some(record) = self.pick_reserve_record(rng) else {
            return;
        };
        let before = self.expected[&record.account];
        let reservation = self
            .ledger
            .reserve(
                CALLER,
                &record.account,
                &record.sku,
                record.amount,
                &record.key,
            )
            .expect("an exact recorded reserve replay succeeds");
        assert!(
            reservation.replayed,
            "a recorded key replays, not double-locks"
        );
        assert_eq!(
            self.ledger.balances(&record.account).unwrap(),
            before,
            "a reserve replay moves no money"
        );
        self.stats.reserve_replays += 1;
        self.verify(step, "reserve replay");
    }

    fn do_reserve_conflict(&mut self, rng: &mut XorShift64, step: u32) {
        let Some(record) = self.pick_reserve_record(rng) else {
            return;
        };
        let changed = record
            .amount
            .checked_add(Amount::from_atomic(1))
            .expect("conflict amount");
        let before = self.expected[&record.account];
        match self
            .ledger
            .reserve(CALLER, &record.account, &record.sku, changed, &record.key)
        {
            Err(CoreError::IdempotencyConflict(_)) => {}
            other => panic!("same key with a changed payload must conflict, got {other:?}"),
        }
        assert_eq!(
            self.ledger.balances(&record.account).unwrap(),
            before,
            "a conflicted reserve moves no money"
        );
        self.stats.reserve_conflicts += 1;
        self.verify(step, "reserve conflict");
    }

    fn pick_reserve_record(&self, rng: &mut XorShift64) -> Option<ReserveRecord> {
        if self.reserve_records.is_empty() {
            return None;
        }
        let index = rng.below(self.reserve_records.len() as u64) as usize;
        Some(self.reserve_records[index].clone())
    }

    fn do_start(&mut self, rng: &mut XorShift64, step: u32) {
        let Some(job) = self.pick_job(rng) else {
            return;
        };
        let state = self.ledger.job_state(&job).expect("job exists");
        match self.ledger.start(&job) {
            Ok(returned) => {
                assert_eq!(returned, JobState::Running);
                assert!(
                    matches!(state, JobState::Reserved | JobState::Running),
                    "start from {state:?} cannot succeed"
                );
                self.stats.starts += 1;
            }
            Err(CoreError::IllegalTransition { .. }) => {
                assert!(
                    state.is_terminal(),
                    "start may only fail from a terminal state, was {state:?}"
                );
                self.stats.start_rejections += 1;
            }
            other => panic!("unexpected start outcome: {other:?}"),
        }
        self.verify(step, "start");
    }

    fn do_settle(&mut self, rng: &mut XorShift64, step: u32) {
        let Some(job) = self.pick_job(rng) else {
            return;
        };
        let snapshot = self.ledger.job_snapshot(&job).expect("job snapshot");
        let account = snapshot.account.clone();
        let amount = snapshot.amount;
        let before = self.expected[&account];
        // On a settled job, half the attempts replay the bound hash and
        // half attack with a different one.
        let hash = match self.settled_hashes.get(&job) {
            Some(bound) if rng.below(2) == 0 => *bound,
            _ => random_hash(rng),
        };
        match self.ledger.settle(&job, hash) {
            Ok(settlement) => match snapshot.state {
                JobState::Running => {
                    assert!(!settlement.replayed, "a running job settles freshly");
                    assert_eq!(settlement.charged, amount);
                    assert_eq!(settlement.report_hash, hash);
                    let mut after = before;
                    after.reserved = before
                        .reserved
                        .checked_sub(amount)
                        .expect("shadow settle sub");
                    after.settled_total = before
                        .settled_total
                        .checked_add(amount)
                        .expect("shadow settle add");
                    self.expected.insert(account.clone(), after);
                    self.settled_hashes.insert(job.clone(), hash);
                    self.settled_jobs.push(job.clone());
                    self.stats.settles += 1;
                }
                JobState::Settled => {
                    // Only the bound hash can replay; money never moves twice.
                    assert!(settlement.replayed, "a settled job never settles twice");
                    assert_eq!(
                        settlement.report_hash,
                        *self.settled_hashes.get(&job).expect("bound hash"),
                        "settle binds exactly one hash per job"
                    );
                    assert_eq!(
                        self.ledger.balances(&account).unwrap(),
                        before,
                        "a settle replay moves no money"
                    );
                    self.stats.settle_replays += 1;
                }
                other => panic!("settle from {other:?} cannot succeed"),
            },
            Err(CoreError::IllegalTransition { .. }) => {
                match snapshot.state {
                    JobState::Settled => {
                        assert_ne!(
                            hash,
                            *self.settled_hashes.get(&job).expect("bound hash"),
                            "only a different hash is rejected on a settled job"
                        );
                    }
                    JobState::Reserved | JobState::Refunded => {}
                    other => panic!("settle rejection from {other:?}"),
                }
                assert_eq!(
                    self.ledger.balances(&account).unwrap(),
                    before,
                    "a rejected settle moves no money"
                );
                self.stats.settle_rejections += 1;
            }
            other => panic!("unexpected settle outcome: {other:?}"),
        }
        self.verify(step, "settle");
    }

    fn do_settle_hash_replay(&mut self, rng: &mut XorShift64, step: u32) {
        if self.settled_jobs.is_empty() {
            return;
        }
        let index = rng.below(self.settled_jobs.len() as u64) as usize;
        let job = self.settled_jobs[index].clone();
        let bound = self.settled_hashes[&job];
        let snapshot = self.ledger.job_snapshot(&job).expect("job snapshot");
        let account = snapshot.account.clone();
        let before = self.expected[&account];
        let settlement = self
            .ledger
            .settle(&job, bound)
            .expect("the bound hash always replays");
        assert!(
            settlement.replayed,
            "the bound hash replays, not re-charges"
        );
        assert_eq!(settlement.report_hash, bound);
        assert_eq!(
            self.ledger.balances(&account).unwrap(),
            before,
            "a bound-hash replay moves no money"
        );
        self.stats.settle_replays += 1;
        self.verify(step, "settle hash replay");
    }

    fn do_refund(&mut self, rng: &mut XorShift64, step: u32) {
        let Some(job) = self.pick_job(rng) else {
            return;
        };
        let snapshot = self.ledger.job_snapshot(&job).expect("job snapshot");
        let account = snapshot.account.clone();
        let amount = snapshot.amount;
        let before = self.expected[&account];
        match self.ledger.refund(&job, "fuzz-refund") {
            Ok(receipt) => match snapshot.state {
                JobState::Reserved | JobState::Running => {
                    assert!(!receipt.replayed, "the first refund moves money once");
                    assert_eq!(receipt.refunded, amount);
                    let mut after = before;
                    after.reserved = before
                        .reserved
                        .checked_sub(amount)
                        .expect("shadow refund sub");
                    after.available = before
                        .available
                        .checked_add(amount)
                        .expect("shadow refund add");
                    after.refunded_total = before
                        .refunded_total
                        .checked_add(amount)
                        .expect("shadow refund counter");
                    self.expected.insert(account.clone(), after);
                    self.stats.refunds += 1;
                }
                JobState::Refunded => {
                    // Refunds are one-shot: a second refund replays only.
                    assert!(receipt.replayed, "a refunded job never refunds twice");
                    assert_eq!(receipt.refunded, amount);
                    assert_eq!(
                        self.ledger.balances(&account).unwrap(),
                        before,
                        "a refund replay moves no money"
                    );
                    self.stats.refund_replays += 1;
                }
                other => panic!("refund from {other:?} cannot succeed"),
            },
            Err(CoreError::IllegalTransition { .. }) => {
                assert_eq!(
                    snapshot.state,
                    JobState::Settled,
                    "only a settled job refuses a refund"
                );
                assert_eq!(
                    self.ledger.balances(&account).unwrap(),
                    before,
                    "a rejected refund moves no money"
                );
                self.stats.refund_rejections += 1;
            }
            other => panic!("unexpected refund outcome: {other:?}"),
        }
        self.verify(step, "refund");
    }
}

/// Drive `steps` random interleaved operations over four accounts and
/// return the coverage counters.
fn run_fuzz(seed: u64, steps: u32) -> FuzzStats {
    let accounts = ["fuzz-acct-a", "fuzz-acct-b", "fuzz-acct-c", "fuzz-acct-d"];
    let mut state = FuzzState::new(&accounts);
    let mut rng = XorShift64::new(seed);
    for step in 0..steps {
        let pick = rng.below(100);
        match pick {
            0..=13 => state.do_deposit(&mut rng, step),
            14..=23 => state.do_deposit_replay(&mut rng, step),
            24..=31 => state.do_deposit_conflict(&mut rng, step),
            32..=49 => state.do_reserve(&mut rng, step),
            50..=57 => state.do_reserve_replay(&mut rng, step),
            58..=65 => state.do_reserve_conflict(&mut rng, step),
            66..=75 => state.do_start(&mut rng, step),
            76..=87 => state.do_settle(&mut rng, step),
            88..=95 => state.do_refund(&mut rng, step),
            _ => state.do_settle_hash_replay(&mut rng, step),
        }
    }
    state.verify(steps, "final");
    assert!(!state.jobs.is_empty(), "the fuzz must create jobs");
    assert!(!state.settled_jobs.is_empty(), "the fuzz must settle jobs");
    state.stats
}

fn assert_all_classes_exercised(stats: &FuzzStats) {
    assert!(stats.deposits > 0, "deposits");
    assert!(stats.deposit_replays > 0, "deposit replays");
    assert!(stats.deposit_conflicts > 0, "deposit conflicts");
    assert!(stats.reserves > 0, "reserves");
    assert!(stats.reserve_replays > 0, "reserve replays");
    assert!(stats.reserve_conflicts > 0, "reserve conflicts");
    assert!(stats.insufficient > 0, "insufficient-funds refusals");
    assert!(stats.starts > 0, "starts");
    assert!(stats.start_rejections > 0, "start rejections");
    assert!(stats.settles > 0, "settles");
    assert!(stats.settle_replays > 0, "settle replays");
    assert!(stats.settle_rejections > 0, "settle rejections");
    assert!(stats.refunds > 0, "refunds");
    assert!(
        stats.refund_replays > 0,
        "refund replays (one-shot refunds)"
    );
    assert!(stats.refund_rejections > 0, "refund rejections");
}

#[test]
fn conservation_holds_under_deep_random_interleavings() {
    let stats = run_fuzz(0x2545_F491_4F6C_DD1D, 2400);
    assert_all_classes_exercised(&stats);
}

#[test]
fn conservation_holds_under_a_second_seed() {
    let stats = run_fuzz(0x9E37_79B9_7F4A_7C15, 2400);
    assert_all_classes_exercised(&stats);
}

/// Overflow edges: deposits that would cross u128::MAX are typed Overflow
/// errors, never panics, never partial credits, and never conservation
/// breaks.
#[test]
fn overflow_deposits_are_errors_not_panics() {
    let mut ledger = Ledger::new();

    // Near the ceiling: lands, conserves.
    let near_max = Amount::from_atomic(u128::MAX - 5);
    ledger
        .deposit(CALLER, "overflow-acct", near_max, "ovf-1")
        .expect("a deposit below the ceiling succeeds");
    assert_eq!(
        ledger.balances("overflow-acct").unwrap().deposited_total,
        near_max
    );
    assert!(ledger.is_conserved());

    // Ten more units cross the ceiling: typed error, no partial credit.
    match ledger.deposit(CALLER, "overflow-acct", Amount::from_atomic(10), "ovf-2") {
        Err(CoreError::Overflow) => {}
        other => panic!("crossing u128::MAX must be an Overflow error, got {other:?}"),
    }
    assert!(
        ledger.is_conserved(),
        "a failed deposit must not break conservation"
    );
    let after_refusal = ledger.balances("overflow-acct").unwrap();
    assert_eq!(after_refusal.deposited_total, near_max);
    assert_eq!(after_refusal.available, near_max);

    // Topping up exactly to the ceiling is still fine.
    ledger
        .deposit(CALLER, "overflow-acct", Amount::from_atomic(5), "ovf-3")
        .expect("reaching exactly u128::MAX succeeds");
    let at_max = ledger.balances("overflow-acct").unwrap();
    assert_eq!(at_max.deposited_total, Amount::from_atomic(u128::MAX));
    assert!(at_max.is_conserved());
    assert!(ledger.is_conserved());

    // One unit past the ceiling: typed error again, nothing moves.
    match ledger.deposit(CALLER, "overflow-acct", Amount::from_atomic(1), "ovf-4") {
        Err(CoreError::Overflow) => {}
        other => panic!("one unit past u128::MAX must be an Overflow error, got {other:?}"),
    }
    assert_eq!(
        ledger.balances("overflow-acct").unwrap().deposited_total,
        Amount::from_atomic(u128::MAX)
    );
    assert!(ledger.is_conserved());

    // A second account at the ceiling is independent, and money still
    // moves: reserve, start, settle all work at the extremes.
    ledger
        .deposit(
            CALLER,
            "overflow-acct-2",
            Amount::from_atomic(u128::MAX),
            "ovf-5",
        )
        .expect("a second account at the ceiling succeeds");
    assert!(ledger.is_conserved());

    let reservation = ledger
        .reserve(
            CALLER,
            "overflow-acct",
            SKU,
            Amount::from_atomic(1),
            "ovf-res",
        )
        .expect("reserving from a full account succeeds");
    assert!(ledger.is_conserved());
    ledger.start(&reservation.job_id).expect("start succeeds");
    let settlement = ledger
        .settle(&reservation.job_id, ReportHash([7u8; 32]))
        .expect("settle at the extremes succeeds");
    assert_eq!(settlement.charged, Amount::from_atomic(1));
    let spent = ledger.balances("overflow-acct").unwrap();
    assert_eq!(spent.available, Amount::from_atomic(u128::MAX - 1));
    assert_eq!(spent.settled_total, Amount::from_atomic(1));
    assert_eq!(spent.reserved, Amount::ZERO);
    assert!(spent.is_conserved());

    // Terminal settle still refuses refunds with a typed error.
    assert!(matches!(
        ledger.refund(&reservation.job_id, "regret"),
        Err(CoreError::IllegalTransition { .. })
    ));
    assert!(ledger.is_conserved());
}
