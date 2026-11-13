//! Property tests over the wallet's real methods (near-sdk mock runtime).
//!
//! Each step is applied to the contract and to a shadow that uses
//! `policy-core` for the arithmetic. A refusal must leave both unchanged.
//! After every step:
//!
//! - the stored window total is at most the daily cap, and it equals the
//!   sum of live reservations in that window (held, in flight, and settled);
//! - no live reservation is above the per-call cap, or for a zero amount;
//! - an in-flight or settled payment is to an allowlisted payee, unless the
//!   owner approved a held unknown payee;
//! - a revoked agent gains no new reservation;
//! - a hold creates no `ft_transfer` receipt;
//! - `approve_payment` and `reject_payment` from anyone but the owner leave
//!   the queue untouched, and each payment id is charged at most once;
//! - the token-out total (in flight + settled) changes only when a transfer
//!   is scheduled or a failed callback releases it.

use super::*;
use near_sdk::test_utils::{get_created_receipts, VMContextBuilder};
use near_sdk::{testing_env, PromiseResult, RuntimeFeesConfig};
use policy_core::{AgentPolicy, ReleaseOutcome, WalletGate, NANOS_PER_DAY};
use proptest::prelude::*;
use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};

const PER_CALL: u128 = 50;
const DAILY: u128 = 200;
const T0: u64 = 20_758 * NANOS_PER_DAY;

fn acc(name: &str) -> AccountId {
    name.parse().unwrap()
}
fn wallet() -> AccountId {
    acc("agent-wallet.owner.near")
}
fn owner() -> AccountId {
    acc("owner.near")
}
fn agent() -> AccountId {
    acc("agent.near")
}
fn gateway() -> AccountId {
    acc("gateway.near")
}
fn stranger() -> AccountId {
    acc("stranger.near")
}
fn usdc() -> AccountId {
    acc("usdc.near")
}
fn other_token() -> AccountId {
    acc("other.near")
}

fn set_ctx(predecessor: &AccountId, deposit: u128, ts: u64) {
    testing_env!(VMContextBuilder::new()
        .current_account_id(wallet())
        .signer_account_id(predecessor.clone())
        .predecessor_account_id(predecessor.clone())
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .block_timestamp(ts)
        .prepaid_gas(Gas::from_tgas(300))
        .build());
}

/// `testing_env!` carries the previous case's trie forward. Drop it so each
/// proptest case starts from an empty chain.
fn wipe_storage() {
    testing_env!(VMContextBuilder::new().build());
    near_sdk::mock::with_mocked_blockchain(|chain| {
        chain.take_storage();
    });
}

fn set_ctx_with_result(predecessor: &AccountId, ts: u64, result: PromiseResult) {
    testing_env!(
        VMContextBuilder::new()
            .current_account_id(wallet())
            .predecessor_account_id(predecessor.clone())
            .block_timestamp(ts)
            .prepaid_gas(Gas::from_tgas(300))
            .build(),
        near_sdk::test_vm_config(),
        RuntimeFeesConfig::test(),
        std::collections::HashMap::default(),
        vec![result]
    );
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_string())
}

#[derive(Clone, Copy, Debug)]
enum PayeePick {
    Allowlisted,
    Stranger,
}

#[derive(Clone, Debug)]
enum Op {
    Pay {
        amount: u128,
        payee: PayeePick,
        bad_token: bool,
    },
    Advance {
        days: u64,
    },
    Review {
        threshold: u128,
        unknown: bool,
    },
    Approve {
        owner: bool,
        index: usize,
    },
    Reject {
        owner: bool,
        index: usize,
    },
    Revoke,
    Restore,
    Pause {
        paused: bool,
    },
    Finish {
        success: bool,
        index: usize,
    },
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (0u128..=250, any::<bool>()).prop_map(|(amount, stranger)| Op::Pay {
            amount,
            payee: if stranger { PayeePick::Stranger } else { PayeePick::Allowlisted },
            bad_token: false,
        }),
        1 => (1u128..=40).prop_map(|amount| Op::Pay {
            amount,
            payee: PayeePick::Allowlisted,
            bad_token: true,
        }),
        2 => (0u64..=3).prop_map(|days| Op::Advance { days }),
        1 => (prop_oneof![Just(0u128), Just(25), Just(50)], any::<bool>()).prop_map(
            |(threshold, unknown)| Op::Review { threshold, unknown }
        ),
        2 => (any::<bool>(), 0usize..12).prop_map(|(owner, index)| Op::Approve { owner, index }),
        2 => (any::<bool>(), 0usize..12).prop_map(|(owner, index)| Op::Reject { owner, index }),
        1 => Just(Op::Revoke),
        1 => Just(Op::Restore),
        1 => any::<bool>().prop_map(|paused| Op::Pause { paused }),
        2 => (any::<bool>(), 0usize..6).prop_map(|(success, index)| Op::Finish { success, index }),
    ]
}

#[derive(Clone, Debug)]
struct Record {
    id: u64,
    amount: u128,
    window: u64,
    payee: String,
    /// The payee was on the allowlist when the payment was reserved.
    allowlisted: bool,
    /// The owner approved a hold of a payee that was not allowlisted.
    approved_unlisted: bool,
    inflight: bool,
}

#[derive(Clone, Debug)]
struct Shadow {
    policy: AgentPolicy,
    now: u64,
    paused: bool,
    threshold: u128,
    review_unknown: bool,
    next_id: u64,
    held: Vec<Record>,
    live: Vec<Record>,
    charged: HashSet<u64>,
}

impl Shadow {
    fn new() -> Self {
        Self {
            policy: AgentPolicy::new(PER_CALL, DAILY, vec![gateway().to_string()]).unwrap(),
            now: T0,
            paused: false,
            threshold: 0,
            review_unknown: false,
            next_id: 0,
            held: Vec::new(),
            live: Vec::new(),
            charged: HashSet::new(),
        }
    }

    fn gate(&self, token_ok: bool) -> WalletGate {
        WalletGate {
            paused: self.paused,
            token_allowed: token_ok,
        }
    }

    fn window_sum(&self) -> u128 {
        let window = self.policy.window_index();
        self.held
            .iter()
            .chain(self.live.iter())
            .filter(|record| record.window == window)
            .map(|record| record.amount)
            .sum()
    }
}

enum Decision {
    Hold {
        policy: AgentPolicy,
        window: u64,
        unlisted: bool,
    },
    Inflight {
        policy: AgentPolicy,
        window: u64,
    },
}

fn decide_pay(shadow: &Shadow, amount: u128, payee: &str, token_ok: bool) -> Result<Decision, ()> {
    if shadow.paused {
        return Err(());
    }
    let allowlisted = shadow.policy.is_payee_allowed(payee);
    let hold_new = shadow.review_unknown && !allowlisted;
    let hold_amount = allowlisted && shadow.threshold > 0 && amount > shadow.threshold;
    let gate = shadow.gate(token_ok);
    if hold_new || hold_amount {
        if shadow.held.len() >= MAX_PENDING {
            return Err(());
        }
        let mut policy = shadow.policy.clone();
        let reserved = if hold_new {
            policy.check_and_reserve_unlisted(gate, amount, shadow.now)
        } else {
            policy.check_and_reserve(gate, payee, amount, shadow.now)
        };
        return match reserved {
            Ok(reservation) => Ok(Decision::Hold {
                policy,
                window: reservation.window_index,
                unlisted: hold_new,
            }),
            Err(_) => Err(()),
        };
    }
    let mut policy = shadow.policy.clone();
    match policy.check_and_reserve(gate, payee, amount, shadow.now) {
        Ok(reservation) => Ok(Decision::Inflight {
            policy,
            window: reservation.window_index,
        }),
        Err(_) => Err(()),
    }
}

fn fresh_wallet() -> PolicyWallet {
    wipe_storage();
    set_ctx(&wallet(), 0, T0);
    let mut w = PolicyWallet::new(owner(), vec![usdc()]);
    set_ctx(&owner(), 1, T0);
    w.set_agent(agent(), U128(PER_CALL), U128(DAILY), vec![gateway()]);
    w
}

fn scheduled_transfer() -> bool {
    get_created_receipts().iter().any(|receipt| {
        receipt.receiver_id == usdc()
            && receipt.actions.iter().any(|action| match action {
                near_sdk::mock::MockAction::FunctionCallWeight {
                    method_name,
                    attached_deposit,
                    ..
                } => {
                    method_name.as_slice() == b"ft_transfer" && attached_deposit.as_yoctonear() == 1
                }
                _ => false,
            })
    })
}

fn apply(w: &mut PolicyWallet, shadow: &mut Shadow, op: Op) {
    match op {
        Op::Pay {
            amount,
            payee,
            bad_token,
        } => apply_pay(w, shadow, amount, payee, bad_token),
        Op::Advance { days } => {
            shadow.now = shadow
                .now
                .saturating_add(days.saturating_mul(NANOS_PER_DAY));
        }
        Op::Review { threshold, unknown } => apply_review(w, shadow, threshold, unknown),
        Op::Approve {
            owner: is_owner,
            index,
        } => apply_approve(w, shadow, is_owner, index),
        Op::Reject {
            owner: is_owner,
            index,
        } => apply_reject(w, shadow, is_owner, index),
        Op::Revoke => apply_active(w, shadow, false),
        Op::Restore => apply_active(w, shadow, true),
        Op::Pause { paused } => apply_pause(w, shadow, paused),
        Op::Finish { success, index } => apply_finish(w, shadow, success, index),
    }
    assert_invariants(w, shadow);
}

fn apply_pay(
    w: &mut PolicyWallet,
    shadow: &mut Shadow,
    amount: u128,
    pick: PayeePick,
    bad_token: bool,
) {
    let payee = match pick {
        PayeePick::Allowlisted => gateway(),
        PayeePick::Stranger => stranger(),
    };
    let token = if bad_token { other_token() } else { usdc() };
    let decision = decide_pay(shadow, amount, payee.as_str(), !bad_token);
    let was_active = shadow.policy.active();
    set_ctx(&agent(), 0, shadow.now);
    let next_id = w.get_config().next_payment_id.0;
    let caught = catch_unwind(AssertUnwindSafe(|| {
        match w.pay(token, payee.clone(), U128(amount), None) {
            PromiseOrValue::Value(U64(id)) => Some(id),
            PromiseOrValue::Promise(promise) => {
                promise.detach();
                None
            }
        }
    }))
    .map_err(panic_text);
    match (decision, caught) {
        (Err(()), Err(_)) => {}
        (
            Ok(Decision::Hold {
                policy,
                window,
                unlisted,
            }),
            Ok(Some(id)),
        ) => {
            assert_eq!(id, next_id);
            assert!(was_active, "a revoked agent must not reserve");
            assert!(!scheduled_transfer(), "a hold must not transfer");
            shadow.policy = policy;
            shadow.next_id = id + 1;
            shadow.held.push(Record {
                id,
                amount,
                window,
                payee: payee.to_string(),
                allowlisted: !unlisted,
                approved_unlisted: false,
                inflight: false,
            });
        }
        (Ok(Decision::Inflight { policy, window }), Ok(None)) => {
            assert!(was_active, "a revoked agent must not settle");
            assert!(
                scheduled_transfer(),
                "a direct pay must schedule ft_transfer"
            );
            assert!(
                shadow.policy.is_payee_allowed(payee.as_str()),
                "a direct settlement requires an allowlisted payee"
            );
            assert!(shadow.charged.insert(next_id), "payment id charged twice");
            shadow.policy = policy;
            shadow.next_id = next_id + 1;
            shadow.live.push(Record {
                id: next_id,
                amount,
                window,
                payee: payee.to_string(),
                allowlisted: true,
                approved_unlisted: false,
                inflight: true,
            });
        }
        (expected, got) => panic!("pay diverged: expected {expected:?} got {got:?}"),
    }
}

impl std::fmt::Debug for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Decision::Hold {
                unlisted, window, ..
            } => {
                write!(f, "Hold {{ unlisted: {unlisted}, window: {window} }}")
            }
            Decision::Inflight { window, .. } => write!(f, "Inflight {{ window: {window} }}"),
        }
    }
}

fn apply_review(w: &mut PolicyWallet, shadow: &mut Shadow, threshold: u128, unknown: bool) {
    set_ctx(&owner(), 1, shadow.now);
    w.set_review_policy(U128(threshold), unknown);
    shadow.threshold = threshold;
    shadow.review_unknown = unknown;
    set_ctx(&agent(), 1, shadow.now);
    let before = shadow.clone();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        w.set_review_policy(U128(1), true);
    }));
    assert!(caught.is_err(), "an agent must not set the review policy");
    *shadow = before;
}

/// `None` when the queue is empty or the index lands on the extra "missing id" slot.
fn take_index(records: &[Record], index: usize) -> Option<u64> {
    if records.is_empty() {
        return None;
    }
    let slot = index % (records.len() + 1);
    records.get(slot).map(|record| record.id)
}

fn apply_approve(w: &mut PolicyWallet, shadow: &mut Shadow, is_owner: bool, index: usize) {
    let id = take_index(&shadow.held, index);
    let caller = if is_owner { owner() } else { agent() };
    let before = shadow.clone();
    set_ctx(&caller, 1, shadow.now);
    let caught = catch_unwind(AssertUnwindSafe(|| {
        w.approve_payment(U64(id.unwrap_or(u64::MAX))).detach();
    }));
    if !is_owner || id.is_none() || shadow.paused || !shadow.policy.active() {
        assert!(caught.is_err(), "approve should have been refused");
        assert_eq!(w.get_pending().len(), before.held.len());
        return;
    }
    if let Err(payload) = caught {
        panic!("owner approve failed: {}", panic_text(payload));
    }
    assert!(scheduled_transfer(), "approve must schedule ft_transfer");
    let id = id.unwrap();
    assert!(shadow.charged.insert(id), "payment id charged twice");
    let position = shadow
        .held
        .iter()
        .position(|record| record.id == id)
        .unwrap();
    let mut record = shadow.held.remove(position);
    record.inflight = true;
    record.approved_unlisted = !record.allowlisted;
    shadow.live.push(record);
}

fn apply_reject(w: &mut PolicyWallet, shadow: &mut Shadow, is_owner: bool, index: usize) {
    let id = take_index(&shadow.held, index);
    let caller = if is_owner { owner() } else { agent() };
    set_ctx(&caller, 1, shadow.now);
    let caught = catch_unwind(AssertUnwindSafe(|| {
        w.reject_payment(U64(id.unwrap_or(u64::MAX)));
    }));
    if !is_owner || id.is_none() {
        assert!(caught.is_err(), "reject should have been refused");
        return;
    }
    assert!(caught.is_ok(), "owner reject failed");
    let id = id.unwrap();
    let position = shadow
        .held
        .iter()
        .position(|record| record.id == id)
        .unwrap();
    let record = shadow.held.remove(position);
    let outcome = shadow.policy.release(policy_core::Reservation {
        amount: record.amount,
        window_index: record.window,
    });
    assert!(matches!(
        outcome,
        Ok(ReleaseOutcome::Restored | ReleaseOutcome::WindowExpired)
    ));
}

fn apply_active(w: &mut PolicyWallet, shadow: &mut Shadow, restore: bool) {
    set_ctx(&owner(), 1, shadow.now);
    if restore {
        w.set_agent(agent(), U128(PER_CALL), U128(DAILY), vec![gateway()]);
        shadow
            .policy
            .reconfigure(PER_CALL, DAILY, vec![gateway().to_string()])
            .unwrap();
    } else {
        w.revoke_agent(agent());
        shadow.policy.deactivate();
    }
    set_ctx(&agent(), 1, shadow.now);
    let before_active = shadow.policy.active();
    let before_spent = shadow.policy.spent_in_window();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        if restore {
            w.set_agent(agent(), U128(1), U128(1), vec![]);
        } else {
            w.revoke_agent(agent());
        }
    }));
    assert!(
        caught.is_err(),
        "an agent must not change its own active flag"
    );
    assert_eq!(shadow.policy.active(), before_active);
    assert_eq!(shadow.policy.spent_in_window(), before_spent);
}

fn apply_pause(w: &mut PolicyWallet, shadow: &mut Shadow, paused: bool) {
    set_ctx(&owner(), 1, shadow.now);
    if paused {
        w.pause();
    } else {
        w.unpause();
    }
    shadow.paused = paused;
    set_ctx(&agent(), 1, shadow.now);
    let caught = catch_unwind(AssertUnwindSafe(|| {
        if paused {
            w.unpause();
        } else {
            w.pause();
        }
    }));
    assert!(caught.is_err(), "an agent must not pause or unpause");
}

fn apply_finish(w: &mut PolicyWallet, shadow: &mut Shadow, success: bool, index: usize) {
    let Some(id) = take_index(
        &shadow
            .live
            .iter()
            .filter(|record| record.inflight)
            .cloned()
            .collect::<Vec<_>>(),
        index,
    ) else {
        return;
    };
    let position = shadow
        .live
        .iter()
        .position(|record| record.id == id)
        .unwrap();
    let record = shadow.live[position].clone();
    let result = if success {
        PromiseResult::Successful(Vec::new())
    } else {
        PromiseResult::Failed
    };
    set_ctx_with_result(&wallet(), shadow.now, result);
    let settled = w.on_pay_complete(
        agent(),
        usdc(),
        acc(&record.payee),
        U128(record.amount),
        U64(record.id),
        U64(record.window),
    );
    assert_eq!(settled, success);
    if success {
        shadow.live[position].inflight = false;
    } else {
        shadow.live.remove(position);
        let outcome = shadow.policy.release(policy_core::Reservation {
            amount: record.amount,
            window_index: record.window,
        });
        assert!(matches!(
            outcome,
            Ok(ReleaseOutcome::Restored | ReleaseOutcome::WindowExpired)
        ));
    }
}

fn assert_invariants(w: &PolicyWallet, shadow: &Shadow) {
    set_ctx(&agent(), 0, shadow.now);
    assert_eq!(
        w.get_spent(agent()).0,
        shadow.policy.spent_at(shadow.now),
        "spent view diverged"
    );
    assert_eq!(
        shadow.policy.spent_in_window(),
        shadow.window_sum(),
        "window total is not the sum of live reservations"
    );
    assert!(
        shadow.policy.spent_in_window() <= DAILY,
        "stored spend {} exceeds the daily cap",
        shadow.policy.spent_in_window()
    );
    assert!(shadow.policy.spent_at(shadow.now) <= DAILY);
    for record in shadow.held.iter().chain(shadow.live.iter()) {
        assert!(record.amount > 0);
        assert!(
            record.amount <= PER_CALL,
            "reservation {} is above the per-call cap",
            record.amount
        );
    }
    for record in &shadow.live {
        assert!(
            record.allowlisted || record.approved_unlisted,
            "in-flight or settled payment to a non-allowlisted payee"
        );
        assert!(shadow.charged.contains(&record.id));
    }
    let pending = w.get_pending();
    assert_eq!(pending.len(), shadow.held.len());
    for (view, record) in pending.iter().zip(&shadow.held) {
        assert_eq!(view.payment_id.0, record.id);
        assert_eq!(view.amount.0, record.amount);
        assert!(
            !shadow.charged.contains(&record.id),
            "a held id was already charged"
        );
    }
    let agent_view = w.get_agent(agent()).unwrap();
    assert_eq!(agent_view.active, shadow.policy.active());
    assert_eq!(agent_view.per_call_max.0, PER_CALL);
    assert_eq!(agent_view.daily_max.0, DAILY);
    let review = w.get_review_policy();
    assert_eq!(review.approval_threshold.0, shadow.threshold);
    assert_eq!(review.review_unknown_payees, shadow.review_unknown);
    assert_eq!(w.get_config().paused, shadow.paused);
    assert_eq!(w.get_config().next_payment_id.0, shadow.next_id);
    let mut ids = HashSet::new();
    for record in shadow.held.iter().chain(shadow.live.iter()) {
        assert!(ids.insert(record.id), "duplicate live id {}", record.id);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn random_wallet_sessions_keep_the_spend_invariants(
        ops in proptest::collection::vec(op_strategy(), 1..36),
    ) {
        let mut w = fresh_wallet();
        let mut shadow = Shadow::new();
        for op in ops {
            apply(&mut w, &mut shadow, op);
        }
    }
}
