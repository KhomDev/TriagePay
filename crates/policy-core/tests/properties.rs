//! Property tests for the policy core.

use policy_core::{
    AgentPolicy, HoldQuota, PolicyError, ReleaseOutcome, Reservation, WalletGate,
    MAX_DAILY_HOLDS_PER_AGENT, MAX_OPEN_HOLDS_PER_AGENT, NANOS_PER_DAY,
};
use proptest::prelude::*;

const OPEN: WalletGate = WalletGate {
    paused: false,
    token_allowed: true,
};
const PAYEE: &str = "gateway.near";

/// One step of a random agent session.
#[derive(Debug, Clone)]
enum Op {
    /// Try to pay `amount`.
    Pay(u128),
    /// The downstream transfer of the n-th outstanding reservation failed.
    Fail(usize),
    /// Time moves forward by this many nanoseconds.
    Advance(u64),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0u128..=120_000).prop_map(Op::Pay),
        2 => any::<usize>().prop_map(Op::Fail),
        1 => (0u64..=2 * NANOS_PER_DAY).prop_map(Op::Advance),
    ]
}

proptest! {
    /// Random sequences of pay, downstream failure and time advance never
    /// leave a window total above `daily_max`, and the sum of outstanding
    /// reservations in the current window always equals the window total.
    #[test]
    fn spend_never_exceeds_daily_max(
        per_call in 1u128..=60_000,
        extra in 0u128..=300_000,
        start in 0u64..=1_000 * NANOS_PER_DAY,
        ops in proptest::collection::vec(op(), 1..200),
    ) {
        let daily = per_call + extra;
        let mut p = AgentPolicy::new(per_call, daily, vec![PAYEE.to_string()]).unwrap();
        let mut now = start;
        let mut outstanding: Vec<Reservation> = Vec::new();
        for op in ops {
            match op {
                Op::Pay(amount) => {
                    let before = p.clone();
                    match p.check_and_reserve(OPEN, PAYEE, amount, now) {
                        Ok(r) => {
                            prop_assert!(amount <= per_call);
                            outstanding.push(r);
                        }
                        Err(e) => {
                            prop_assert!(matches!(
                                e,
                                PolicyError::ZeroAmount | PolicyError::PerCallLimit | PolicyError::DailyLimit
                            ));
                            prop_assert_eq!(&p, &before);
                        }
                    }
                }
                Op::Fail(i) => {
                    if !outstanding.is_empty() {
                        let r = outstanding.remove(i % outstanding.len());
                        prop_assert!(p.release(r).is_ok());
                    }
                }
                Op::Advance(dt) => now = now.saturating_add(dt),
            }
            prop_assert!(p.spent_at(now) <= daily);
            prop_assert!(p.spent_in_window() <= daily);
            let live: u128 = outstanding
                .iter()
                .filter(|r| r.window_index == p.window_index())
                .map(|r| r.amount)
                .sum();
            prop_assert_eq!(live, p.spent_in_window());
        }
    }

    /// Reserving and then releasing restores the exact prior state, for any
    /// policy already positioned in the payment's window.
    #[test]
    fn reserve_then_release_restores_exact_state(
        per_call in 1u128..=u128::MAX / 4,
        extra in 0u128..=u128::MAX / 4,
        spent_frac in 0u128..=1_000,
        day in 0u64..=100_000,
        offset in 0u64..NANOS_PER_DAY,
        amount in 1u128..=u128::MAX / 4,
        active in any::<bool>(),
    ) {
        let daily = per_call + extra;
        let spent = daily / 1_000 * spent_frac;
        let now = day * NANOS_PER_DAY + offset;
        let mut p = AgentPolicy::from_parts(per_call, daily, vec![PAYEE.to_string()], active, spent, day).unwrap();
        let before = p.clone();
        match p.check_and_reserve(OPEN, PAYEE, amount, now) {
            Ok(r) => {
                prop_assert_eq!(r.window_index, day);
                prop_assert_eq!(p.release(r), Ok(ReleaseOutcome::Restored));
                prop_assert_eq!(p, before);
            }
            Err(_) => prop_assert_eq!(p, before),
        }
    }

    /// Crossing a UTC midnight resets the total to zero and moves the window
    /// to the new day; staying inside the day changes nothing.
    #[test]
    fn rollover_resets_correctly(
        spent in 0u128..=200_000,
        day in 0u64..=100_000,
        offset in 0u64..NANOS_PER_DAY,
        later in 0u64..=10 * NANOS_PER_DAY,
    ) {
        let mut p = AgentPolicy::from_parts(50_000, 200_000, vec![PAYEE.to_string()], true, spent, day).unwrap();
        let now = day * NANOS_PER_DAY + offset + later;
        let new_day = now / NANOS_PER_DAY;
        p.rollover(now);
        if new_day > day {
            prop_assert_eq!(p.window_index(), new_day);
            prop_assert_eq!(p.spent_in_window(), 0);
            prop_assert_eq!(p.remaining_at(now), 200_000);
        } else {
            prop_assert_eq!(p.window_index(), day);
            prop_assert_eq!(p.spent_in_window(), spent);
        }
        let snapshot = p.clone();
        p.rollover(now);
        prop_assert_eq!(p, snapshot);
    }
}

/// One step of a session that mixes settled payments with holds.
#[derive(Debug, Clone)]
enum HoldOp {
    /// Settle (reserve) `amount` to the allowlisted payee.
    Pay(u128),
    /// Check a hold of `amount` and, when the check and the quota allow it,
    /// remember the amount without reserving it.
    Hold(u128),
    /// Approve the n-th open hold: reserve it now, or leave it queued when
    /// the cap no longer allows it.
    Approve(usize),
    /// Drop the n-th open hold. Spend does not change.
    Reject(usize),
    /// The downstream transfer of the n-th outstanding reservation failed.
    Fail(usize),
    /// Time moves forward by this many nanoseconds.
    Advance(u64),
}

fn hold_op() -> impl Strategy<Value = HoldOp> {
    prop_oneof![
        3 => (0u128..=80_000).prop_map(HoldOp::Pay),
        3 => (0u128..=80_000).prop_map(HoldOp::Hold),
        2 => any::<usize>().prop_map(HoldOp::Approve),
        2 => any::<usize>().prop_map(HoldOp::Reject),
        1 => any::<usize>().prop_map(HoldOp::Fail),
        1 => (0u64..=2 * NANOS_PER_DAY).prop_map(HoldOp::Advance),
    ]
}

proptest! {
    /// Holds never increase the window total. Approving a hold reserves it
    /// under the same caps as a direct payment. Open holds stay within the
    /// per-agent cap, and a UTC day cannot create more than the quota.
    #[test]
    fn holds_never_consume_the_cap_until_approved(
        per_call in 1u128..=40_000,
        extra in 0u128..=200_000,
        start in 0u64..=1_000 * NANOS_PER_DAY,
        ops in proptest::collection::vec(hold_op(), 1..160),
    ) {
        let daily = per_call + extra;
        let mut policy = AgentPolicy::new(per_call, daily, vec![PAYEE.to_string()]).unwrap();
        let mut quota = HoldQuota::new();
        let mut now = start;
        let mut outstanding: Vec<Reservation> = Vec::new();
        // Amounts sitting in the queue. They are not reservations.
        let mut queued: Vec<u128> = Vec::new();
        let mut created_today: u32 = 0;
        let mut created_window = policy_core::window_index(now);

        for op in ops {
            match op {
                HoldOp::Pay(amount) => {
                    let before = policy.clone();
                    match policy.check_and_reserve(OPEN, PAYEE, amount, now) {
                        Ok(r) => outstanding.push(r),
                        Err(_) => prop_assert_eq!(&policy, &before),
                    }
                }
                HoldOp::Hold(amount) => {
                    let before_policy = policy.clone();
                    let before_quota = quota;
                    let window = policy_core::window_index(now).max(created_window);
                    if window > created_window {
                        created_today = 0;
                        created_window = window;
                    }
                    match policy.check_unlisted(OPEN, amount, now) {
                        Ok(()) => {
                            prop_assert_eq!(&policy, &before_policy);
                            match quota.try_open(now) {
                                Ok(()) => {
                                    prop_assert!(quota.open() <= MAX_OPEN_HOLDS_PER_AGENT);
                                    prop_assert!(
                                        quota.created_in_window() <= MAX_DAILY_HOLDS_PER_AGENT
                                    );
                                    created_today += 1;
                                    prop_assert!(created_today <= MAX_DAILY_HOLDS_PER_AGENT);
                                    queued.push(amount);
                                }
                                Err(e) => {
                                    prop_assert!(matches!(
                                        e,
                                        PolicyError::AgentOpenHolds | PolicyError::AgentDailyHolds
                                    ));
                                    prop_assert_eq!(quota, before_quota);
                                }
                            }
                        }
                        Err(_) => {
                            prop_assert_eq!(&policy, &before_policy);
                            prop_assert_eq!(quota, before_quota);
                        }
                    }
                }
                HoldOp::Approve(i) => {
                    if !queued.is_empty() {
                        let amount = queued.remove(i % queued.len());
                        quota.close_one();
                        let before = policy.clone();
                        match policy.check_and_reserve_unlisted(OPEN, amount, now) {
                            Ok(r) => outstanding.push(r),
                            Err(_) => prop_assert_eq!(&policy, &before),
                        }
                    }
                }
                HoldOp::Reject(i) => {
                    if !queued.is_empty() {
                        queued.remove(i % queued.len());
                        let spent = policy.spent_in_window();
                        quota.close_one();
                        prop_assert_eq!(policy.spent_in_window(), spent);
                    }
                }
                HoldOp::Fail(i) => {
                    if !outstanding.is_empty() {
                        let r = outstanding.remove(i % outstanding.len());
                        prop_assert!(policy.release(r).is_ok());
                    }
                }
                HoldOp::Advance(dt) => now = now.saturating_add(dt),
            }
            prop_assert!(policy.spent_at(now) <= daily);
            prop_assert!(policy.spent_in_window() <= daily);
            prop_assert!(quota.open() <= MAX_OPEN_HOLDS_PER_AGENT);
            prop_assert_eq!(quota.open() as usize, queued.len());
            let live: u128 = outstanding
                .iter()
                .filter(|r| r.window_index == policy.window_index())
                .map(|r| r.amount)
                .sum();
            prop_assert_eq!(live, policy.spent_in_window());
        }
    }
}
