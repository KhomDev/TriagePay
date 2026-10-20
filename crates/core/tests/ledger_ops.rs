//! Integration tests for the prepaid ledger: happy paths, idempotency
//! semantics, state-machine edges, and a randomized conservation fuzz.

use aps_core::{Amount, CoreError, JobId, Ledger, ReportHash};

fn usd(whole: u64) -> Amount {
    Amount::from_whole_units(whole, 6).unwrap()
}

fn hash(seed: u8) -> ReportHash {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    ReportHash(bytes)
}

#[test]
fn deposit_reserve_start_settle_happy_path() {
    let mut ledger = Ledger::new();
    let dep = ledger.deposit("agent-1", "acct-1", usd(5), "d1").unwrap();
    assert!(!dep.replayed);
    assert_eq!(ledger.balances("acct-1").unwrap().available, usd(5));

    let res = ledger
        .reserve("agent-1", "acct-1", "scan.v1", usd(1), "r1")
        .unwrap();
    let after_reserve = ledger.balances("acct-1").unwrap();
    assert_eq!(after_reserve.reserved, usd(1));
    assert_eq!(after_reserve.available, usd(4));

    let state = ledger.start(&res.job_id).unwrap();
    assert_eq!(state, aps_core::JobState::Running);

    let settlement = ledger.settle(&res.job_id, hash(7)).unwrap();
    assert_eq!(settlement.charged, usd(1));
    assert!(!settlement.replayed);

    let after = ledger.balances("acct-1").unwrap();
    assert_eq!(after.settled_total, usd(1));
    assert_eq!(after.available, usd(4));
    assert_eq!(after.reserved, Amount::ZERO);
    assert_eq!(
        ledger.job_state(&res.job_id).unwrap(),
        aps_core::JobState::Settled
    );
    assert!(ledger.is_conserved());
}

#[test]
fn deposit_is_idempotent_and_payload_checked() {
    let mut ledger = Ledger::new();
    let first = ledger.deposit("agent-1", "acct-1", usd(5), "d1").unwrap();
    let replay = ledger.deposit("agent-1", "acct-1", usd(5), "d1").unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.credited, first.credited);
    assert_eq!(replay.balances, first.balances);
    assert_eq!(ledger.balances("acct-1").unwrap().deposited_total, usd(5));

    let conflict = ledger.deposit("agent-1", "acct-1", usd(6), "d1");
    assert!(matches!(conflict, Err(CoreError::IdempotencyConflict(_))));
    assert_eq!(ledger.balances("acct-1").unwrap().deposited_total, usd(5));

    // Keys are scoped to the caller: a different caller may reuse the key.
    ledger.deposit("agent-2", "acct-1", usd(1), "d1").unwrap();
    assert_eq!(ledger.balances("acct-1").unwrap().deposited_total, usd(6));

    assert!(matches!(
        ledger.deposit("agent-1", "acct-1", Amount::ZERO, "d2"),
        Err(CoreError::Validation(_))
    ));
    assert!(ledger.is_conserved());
}

#[test]
fn reserve_requires_existing_account_and_funds() {
    let mut ledger = Ledger::new();
    assert!(matches!(
        ledger.reserve("agent-1", "ghost", "scan.v1", usd(1), "r1"),
        Err(CoreError::UnknownAccount(_))
    ));

    ledger.deposit("agent-1", "acct-1", usd(1), "d1").unwrap();
    let before = ledger.balances("acct-1").unwrap();
    let denied = ledger.reserve("agent-1", "acct-1", "scan.v1", usd(2), "r1");
    assert!(matches!(
        denied,
        Err(CoreError::InsufficientFunds { account, needed, available })
            if account == "acct-1" && needed == usd(2) && available == usd(1)
    ));
    let after = ledger.balances("acct-1").unwrap();
    assert_eq!(before, after);

    assert!(matches!(
        ledger.reserve("agent-1", "acct-1", "scan.v1", Amount::ZERO, "r2"),
        Err(CoreError::Validation(_))
    ));
    assert!(ledger.is_conserved());
}

#[test]
fn settle_requires_running_state_and_nonzero_hash() {
    let mut ledger = Ledger::new();
    ledger.deposit("agent-1", "acct-1", usd(2), "d1").unwrap();
    let res = ledger
        .reserve("agent-1", "acct-1", "scan.v1", usd(1), "r1")
        .unwrap();

    assert!(matches!(
        ledger.settle(&res.job_id, hash(1)),
        Err(CoreError::IllegalTransition { .. })
    ));

    ledger.start(&res.job_id).unwrap();
    assert!(matches!(
        ledger.settle(&res.job_id, ReportHash([0u8; 32])),
        Err(CoreError::Validation(_))
    ));

    let s1 = ledger.settle(&res.job_id, hash(3)).unwrap();
    let s2 = ledger.settle(&res.job_id, hash(3)).unwrap();
    assert!(s2.replayed);
    assert_eq!(s2.charged, s1.charged);
    assert_eq!(s2.report_hash, s1.report_hash);
    assert_eq!(ledger.balances("acct-1").unwrap().settled_total, usd(1));

    assert!(matches!(
        ledger.settle(&res.job_id, hash(4)),
        Err(CoreError::IllegalTransition { .. })
    ));
    assert!(ledger.is_conserved());
}

#[test]
fn refund_returns_credit_exactly_once() {
    let mut ledger = Ledger::new();
    ledger.deposit("agent-1", "acct-1", usd(5), "d1").unwrap();
    let res = ledger
        .reserve("agent-1", "acct-1", "scan.v1", usd(2), "r1")
        .unwrap();
    ledger.start(&res.job_id).unwrap();

    let refund = ledger.refund(&res.job_id, "timeout").unwrap();
    assert_eq!(refund.refunded, usd(2));
    let after = ledger.balances("acct-1").unwrap();
    assert_eq!(after.available, usd(5));
    assert_eq!(after.reserved, Amount::ZERO);
    assert_eq!(after.refunded_total, usd(2));

    let replay = ledger.refund(&res.job_id, "timeout-again").unwrap();
    assert!(replay.replayed);
    assert_eq!(ledger.balances("acct-1").unwrap().refunded_total, usd(2));

    assert!(matches!(
        ledger.settle(&res.job_id, hash(9)),
        Err(CoreError::IllegalTransition { .. })
    ));
    assert!(ledger.is_conserved());
}

#[test]
fn settled_job_cannot_be_refunded() {
    let mut ledger = Ledger::new();
    ledger.deposit("agent-1", "acct-1", usd(1), "d1").unwrap();
    let res = ledger
        .reserve("agent-1", "acct-1", "scan.v1", usd(1), "r1")
        .unwrap();
    ledger.start(&res.job_id).unwrap();
    ledger.settle(&res.job_id, hash(1)).unwrap();
    assert!(matches!(
        ledger.refund(&res.job_id, "regret"),
        Err(CoreError::IllegalTransition { .. })
    ));
    assert_eq!(ledger.balances("acct-1").unwrap().settled_total, usd(1));
    assert!(ledger.is_conserved());
}

#[test]
fn report_hash_hex_round_trip() {
    let h = hash(0xAB);
    let hex = h.to_hex();
    assert_eq!(hex.len(), 64);
    assert!(hex.starts_with("ab"));
    let back = ReportHash::from_hex(&hex).unwrap();
    assert_eq!(back, h);
    assert!(ReportHash::from_hex("nothex").is_err());
    assert!(ReportHash::from_hex("ab").is_err());

    let zero_hex = ReportHash([0u8; 32]).to_hex();
    let zero = ReportHash::from_hex(&zero_hex).unwrap();
    assert!(zero.is_zero());
    assert!(!h.is_zero());
}

struct XorShift64(u64);
impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

#[test]
fn conservation_holds_under_random_operation_sequences() {
    let mut rng = XorShift64(0x2545_F491_4F6C_DD1D);
    let accounts = ["a1", "a2", "a3"];
    let mut jobs: Vec<JobId> = Vec::new();
    let mut ledger = Ledger::new();

    for step in 0..2000u32 {
        let pick = rng.next() % 100;
        let acct = accounts[(rng.next() % 3) as usize];
        let amt = usd(1 + (rng.next() % 3) as u64);
        if pick < 30 {
            let key = format!("d{}", step);
            let _ = ledger.deposit("fuzz", acct, amt, &key);
        } else if pick < 55 {
            let key = format!("r{}", step);
            if let Ok(res) = ledger.reserve("fuzz", acct, "scan.v1", amt, &key) {
                jobs.push(res.job_id.clone());
            }
        } else if pick < 70 {
            if let Some(job) = jobs.last() {
                let _ = ledger.start(job);
            }
        } else if pick < 85 {
            if !jobs.is_empty() {
                let idx = (rng.next() as usize) % jobs.len();
                let _ = ledger.settle(&jobs[idx], hash((step % 255) as u8));
            }
        } else {
            if !jobs.is_empty() {
                let idx = (rng.next() as usize) % jobs.len();
                let _ = ledger.refund(&jobs[idx], "fuzz-refund");
            }
        }
        if step % 97 == 0 {
            assert!(ledger.is_conserved(), "conservation broke at step {}", step);
        }
    }
    assert!(ledger.is_conserved());
    assert!(!jobs.is_empty());
}
