//! Unit tests over near-sdk's mocked blockchain.
//!
//! The mocked runtime does not revert state on panic, so "no state change
//! on refusal" is covered by the policy-core property tests and the sandbox
//! integration tests; these tests cover method behaviour, access rules,
//! error codes and events.

use super::*;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::{testing_env, PromiseResult, RuntimeFeesConfig};
use policy_core::NANOS_PER_DAY;

const PER_CALL: u128 = 50_000; // 0.05 USDC at 6 decimals
const DAILY: u128 = 200_000; // 0.20 USDC
/// 2026-11-01T01:00:00Z.
const T0: u64 = 20_758 * NANOS_PER_DAY + 3_600 * 1_000_000_000;
const DAY0: u64 = 20_758;

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
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
fn usdc() -> AccountId {
    acc("usdc.near")
}
fn dai() -> AccountId {
    acc("dai.near")
}

struct Ctx {
    predecessor: AccountId,
    deposit: u128,
    ts: u64,
    gas_tgas: u64,
}

impl Ctx {
    fn new(predecessor: AccountId) -> Self {
        Ctx {
            predecessor,
            deposit: 0,
            ts: T0,
            // Gas accumulates across calls inside one mocked context, and
            // each pay attaches 25 TGas statically; 300 TGas (the protocol
            // per-receipt maximum) leaves room for several pays per test.
            gas_tgas: 300,
        }
    }
    fn deposit(mut self, yocto: u128) -> Self {
        self.deposit = yocto;
        self
    }
    fn at(mut self, ts: u64) -> Self {
        self.ts = ts;
        self
    }
    fn gas(mut self, tgas: u64) -> Self {
        self.gas_tgas = tgas;
        self
    }
    fn build(&self) -> near_sdk::VMContext {
        VMContextBuilder::new()
            .current_account_id(wallet())
            .signer_account_id(self.predecessor.clone())
            .predecessor_account_id(self.predecessor.clone())
            .attached_deposit(NearToken::from_yoctonear(self.deposit))
            .block_timestamp(self.ts)
            .prepaid_gas(Gas::from_tgas(self.gas_tgas))
            .build()
    }
    fn set(self) {
        testing_env!(self.build());
    }
    fn set_with_result(self, result: PromiseResult) {
        self.set_with_results(vec![result]);
    }
    fn set_with_results(self, results: Vec<PromiseResult>) {
        testing_env!(
            self.build(),
            near_sdk::test_vm_config(),
            RuntimeFeesConfig::test(),
            Default::default(),
            results
        );
    }
}

fn as_owner() -> Ctx {
    Ctx::new(owner()).deposit(1)
}
fn as_agent() -> Ctx {
    Ctx::new(agent())
}
fn as_self() -> Ctx {
    Ctx::new(wallet())
}

/// A wallet with USDC allowlisted and one agent limited to the gateway.
fn setup() -> PolicyWallet {
    as_self().set();
    let mut w = PolicyWallet::new(owner(), vec![usdc()]);
    as_owner().set();
    w.set_agent(agent(), U128(PER_CALL), U128(DAILY), vec![gateway()]);
    w
}

fn settle(result: PromiseOrValue<U64>) {
    if let PromiseOrValue::Promise(promise) = result {
        promise.detach();
    }
}

fn pay(w: &mut PolicyWallet, amount: u128) {
    settle(w.pay(usdc(), gateway(), U128(amount), None));
}

fn events() -> Vec<String> {
    get_logs()
        .into_iter()
        .filter_map(|l| l.strip_prefix("EVENT_JSON:").map(str::to_string))
        .collect()
}

fn event_names() -> Vec<String> {
    events()
        .iter()
        .map(|e| {
            let v: near_sdk::serde_json::Value = near_sdk::serde_json::from_str(e).unwrap();
            assert_eq!(v["standard"], "aps_policy_wallet");
            assert_eq!(v["version"], "1.0.0");
            v["event"].as_str().unwrap().to_string()
        })
        .collect()
}

// ----- new ------------------------------------------------------------------

#[test]
fn new_initializes_config() {
    as_self().set();
    // A repeated token is one token.
    let w = PolicyWallet::new(owner(), vec![usdc(), usdc()]);
    let c = w.get_config();
    assert_eq!(c.owner, owner());
    assert!(!c.paused);
    assert_eq!(c.tokens, vec![usdc()]);
    assert_eq!(c.next_payment_id, U64(0));
    assert_eq!(c.event_standard, "aps_policy_wallet");
    assert_eq!(c.event_version, "1.0.0");
}

// `#[private]` is enforced by the generated wasm entry point, which the
// mocked runtime bypasses; the sandbox tests cover `new` and the callbacks
// being private.

#[test]
#[should_panic(expected = "ERR_SELF_ACCOUNT")]
fn new_rejects_wallet_as_owner() {
    as_self().set();
    PolicyWallet::new(wallet(), vec![usdc()]);
}

#[test]
#[should_panic(expected = "ERR_TOO_MANY_TOKENS")]
fn new_rejects_too_many_tokens() {
    as_self().set();
    let tokens = (0..=MAX_TOKENS)
        .map(|i| acc(&format!("t{i}.near")))
        .collect();
    PolicyWallet::new(owner(), tokens);
}

// One wallet, one denomination: limits and the running total are plain
// integers in the token's atomic units, with no per-token bookkeeping.

#[test]
#[should_panic(expected = "ERR_TOO_MANY_TOKENS")]
fn new_rejects_a_second_token() {
    as_self().set();
    PolicyWallet::new(owner(), vec![usdc(), acc("wrap.near")]);
}

#[test]
#[should_panic(expected = "ERR_TOO_MANY_TOKENS")]
fn set_tokens_rejects_a_second_token() {
    let mut w = setup();
    as_owner().set();
    w.set_tokens(vec![usdc(), acc("wrap.near")]);
}

/// The case the rule exists for. Limits sized for a 24-decimal token
/// (0.001 per call, 0.003 per day) are, read as a 6-decimal token's
/// atomic units, 10^15 and 3 x 10^15 whole units. The second token cannot
/// be allowlisted next to the first, so it cannot be paid at all.
#[test]
#[should_panic(expected = "ERR_TOKEN_NOT_ALLOWED")]
fn limits_sized_for_one_token_cannot_move_another() {
    let wnear = acc("wrap.near");
    as_self().set();
    let mut w = PolicyWallet::new(owner(), vec![wnear]);
    as_owner().set();
    let per_call: u128 = 1_000_000_000_000_000_000_000; // 0.001 at 24 decimals
    let daily: u128 = 3_000_000_000_000_000_000_000; // 0.003 at 24 decimals
    w.set_agent(agent(), U128(per_call), U128(daily), vec![gateway()]);
    as_agent().set();
    // One billion USDC in atomic units: far inside the integer limits.
    settle(w.pay(usdc(), gateway(), U128(1_000_000_000_000_000), None));
}

// ----- pay: happy path and events -------------------------------------------

#[test]
fn pay_reserves_schedules_transfer_and_emits() {
    let mut w = setup();
    as_agent().set();
    settle(w.pay(usdc(), gateway(), U128(PER_CALL), Some("job-1".into())));
    assert_eq!(w.get_spent(agent()), U128(PER_CALL));
    assert_eq!(w.get_config().next_payment_id, U64(1));
    assert_eq!(
        events(),
        vec![format!(
            r#"{{"standard":"aps_policy_wallet","version":"1.0.0","event":"pay_requested","data":{{"payment_id":"0","agent_id":"agent.near","token":"usdc.near","receiver_id":"gateway.near","amount":"50000","window_index":"{DAY0}"}}}}"#
        )]
    );
    // ft_transfer to the token, then the callback to the wallet itself.
    let receipts = get_created_receipts();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].receiver_id, usdc());
    assert_eq!(receipts[1].receiver_id, wallet());
    let (method, deposit, args) = function_call(&receipts[0]);
    assert_eq!((method.as_str(), deposit), ("ft_transfer", 1));
    assert_eq!(
        args,
        r#"{"receiver_id":"gateway.near","amount":"50000","memo":"job-1"}"#
    );
    let (method, deposit, _) = function_call(&receipts[1]);
    assert_eq!((method.as_str(), deposit), ("on_pay_complete", 0));
}

/// Method name, attached yoctoNEAR and JSON args of a receipt's single
/// function call.
fn function_call(receipt: &near_sdk::mock::Receipt) -> (String, u128, String) {
    let calls: Vec<_> = receipt
        .actions
        .iter()
        .filter_map(|a| match a {
            near_sdk::mock::MockAction::FunctionCallWeight {
                method_name,
                args,
                attached_deposit,
                ..
            } => Some((
                String::from_utf8(method_name.clone()).unwrap(),
                attached_deposit.as_yoctonear(),
                String::from_utf8(args.clone()).unwrap(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 1, "{:?}", receipt.actions);
    calls.into_iter().next().unwrap()
}

#[test]
fn two_pays_in_the_same_block_both_count() {
    let mut w = setup();
    as_agent().set();
    pay(&mut w, 30_000);
    pay(&mut w, 30_000);
    assert_eq!(w.get_spent(agent()), U128(60_000));
    assert_eq!(w.get_config().next_payment_id, U64(2));
}

// ----- pay: every refusal code ------------------------------------------------

#[test]
#[should_panic(expected = "ERR_PER_CALL_LIMIT")]
fn pay_over_per_call_limit() {
    let mut w = setup();
    as_agent().set();
    pay(&mut w, PER_CALL + 1);
}

#[test]
#[should_panic(expected = "ERR_DAILY_LIMIT")]
fn pay_over_daily_limit() {
    let mut w = setup();
    as_agent().set();
    for _ in 0..4 {
        pay(&mut w, PER_CALL);
    }
    assert_eq!(w.get_agent(agent()).unwrap().remaining, U128(0));
    pay(&mut w, 1);
}

#[test]
#[should_panic(expected = "ERR_PAYEE_NOT_ALLOWED")]
fn pay_to_non_allowlisted_payee() {
    let mut w = setup();
    as_agent().set();
    settle(w.pay(usdc(), acc("attacker.near"), U128(1), None));
}

#[test]
#[should_panic(expected = "ERR_TOKEN_NOT_ALLOWED")]
fn pay_with_disallowed_token() {
    let mut w = setup();
    as_agent().set();
    settle(w.pay(dai(), gateway(), U128(1), None));
}

#[test]
#[should_panic(expected = "ERR_PAUSED")]
fn pay_while_paused() {
    let mut w = setup();
    as_owner().set();
    w.pause();
    as_agent().set();
    pay(&mut w, 1);
}

#[test]
#[should_panic(expected = "ERR_AGENT_INACTIVE")]
fn pay_by_unknown_agent() {
    let mut w = setup();
    Ctx::new(acc("stranger.near")).set();
    pay(&mut w, 1);
}

#[test]
#[should_panic(expected = "ERR_AGENT_INACTIVE")]
fn pay_by_revoked_agent() {
    let mut w = setup();
    as_owner().set();
    w.revoke_agent(agent());
    as_agent().set();
    pay(&mut w, 1);
}

#[test]
#[should_panic(expected = "ERR_ZERO_AMOUNT")]
fn pay_zero_amount() {
    let mut w = setup();
    as_agent().set();
    pay(&mut w, 0);
}

#[test]
#[should_panic(expected = "ERR_DEPOSIT_NOT_ZERO")]
fn pay_with_deposit() {
    let mut w = setup();
    as_agent().deposit(1).set();
    pay(&mut w, 1);
}

#[test]
#[should_panic(expected = "ERR_MEMO_TOO_LONG")]
fn pay_with_long_memo() {
    let mut w = setup();
    as_agent().set();
    settle(w.pay(
        usdc(),
        gateway(),
        U128(1),
        Some("m".repeat(MAX_MEMO_LEN + 1)),
    ));
}

#[test]
#[should_panic(expected = "ERR_INSUFFICIENT_GAS")]
fn pay_with_too_little_gas() {
    let mut w = setup();
    as_agent().gas(30).set();
    pay(&mut w, 1);
}

#[test]
#[should_panic(expected = "ERR_OVERFLOW")]
fn pay_overflow_adjacent_amount() {
    let mut w = setup();
    as_owner().set();
    w.set_agent(agent(), U128(u128::MAX), U128(u128::MAX), vec![gateway()]);
    as_agent().set();
    pay(&mut w, u128::MAX - 1);
    assert_eq!(w.get_spent(agent()), U128(u128::MAX - 1));
    pay(&mut w, 2);
}

// ----- window rollover --------------------------------------------------------

#[test]
fn window_rolls_over_at_utc_midnight() {
    let mut w = setup();
    as_agent().set();
    for _ in 0..4 {
        pay(&mut w, PER_CALL);
    }
    let midnight = (DAY0 + 1) * NANOS_PER_DAY;
    as_agent().at(midnight - 1).set();
    assert_eq!(w.get_agent(agent()).unwrap().remaining, U128(0));
    as_agent().at(midnight).set();
    let view = w.get_agent(agent()).unwrap();
    assert_eq!(view.spent_in_window, U128(0));
    assert_eq!(view.window_index, U64(DAY0 + 1));
    assert_eq!(view.resets_at_ns, U64(midnight + NANOS_PER_DAY));
    pay(&mut w, PER_CALL);
    assert_eq!(w.get_spent(agent()), U128(PER_CALL));
}

// ----- on_pay_complete ----------------------------------------------------------

fn complete(
    w: &mut PolicyWallet,
    result: PromiseResult,
    amount: u128,
    window: u64,
    ts: u64,
) -> bool {
    as_self().at(ts).set_with_result(result);
    w.on_pay_complete(
        agent(),
        usdc(),
        gateway(),
        U128(amount),
        U64(0),
        U64(window),
    )
}

#[test]
fn callback_success_keeps_spend_and_emits_settled() {
    let mut w = setup();
    as_agent().set();
    pay(&mut w, PER_CALL);
    assert!(complete(
        &mut w,
        PromiseResult::Successful(vec![]),
        PER_CALL,
        DAY0,
        T0
    ));
    assert_eq!(w.get_spent(agent()), U128(PER_CALL));
    assert_eq!(event_names(), vec!["pay_settled"]);
}

#[test]
fn callback_failure_releases_the_reservation() {
    let mut w = setup();
    as_agent().set();
    pay(&mut w, 20_000);
    pay(&mut w, PER_CALL);
    assert!(!complete(&mut w, PromiseResult::Failed, PER_CALL, DAY0, T0));
    assert_eq!(w.get_spent(agent()), U128(20_000));
    let logged = events();
    assert_eq!(logged.len(), 1);
    assert!(logged[0].contains(r#""event":"pay_failed""#));
    assert!(logged[0].contains(r#""release":"restored""#));
}

#[test]
fn callback_failure_after_rollover_does_not_touch_new_window() {
    let mut w = setup();
    as_agent().set();
    pay(&mut w, PER_CALL);
    let next_day = (DAY0 + 1) * NANOS_PER_DAY + 10;
    as_agent().at(next_day).set();
    pay(&mut w, 10_000);
    assert!(!complete(
        &mut w,
        PromiseResult::Failed,
        PER_CALL,
        DAY0,
        next_day
    ));
    let logged = events();
    assert_eq!(logged.len(), 1);
    assert!(logged[0].contains(r#""event":"pay_failed""#));
    assert!(logged[0].contains(r#""release":"window_expired""#));
    as_agent().at(next_day).set();
    assert_eq!(w.get_spent(agent()), U128(10_000));
}

#[test]
fn callback_failure_for_revoked_agent_still_releases() {
    let mut w = setup();
    as_agent().set();
    pay(&mut w, PER_CALL);
    as_owner().set();
    w.revoke_agent(agent());
    assert!(!complete(&mut w, PromiseResult::Failed, PER_CALL, DAY0, T0));
    let view = w.get_agent(agent()).unwrap();
    assert!(!view.active);
    assert_eq!(view.spent_in_window, U128(0));
}

#[test]
fn callback_failure_without_reservation_reports_mismatch() {
    let mut w = setup();
    assert!(!complete(&mut w, PromiseResult::Failed, 1, DAY0, T0));
    assert_eq!(w.get_spent(agent()), U128(0));
    assert!(events()[0].contains(r#""release":"mismatch""#));
}

// ----- owner methods: the agent can never reach them -------------------------

macro_rules! agent_cannot_call {
    ($name:ident, $deposit:expr, $expected:literal, |$w:ident| $call:expr) => {
        #[test]
        #[should_panic(expected = $expected)]
        fn $name() {
            let mut $w = setup();
            as_agent().deposit($deposit).set();
            $call;
        }
    };
}

// The near-sdk `assert_one_yocto` message is
// "Requires attached deposit of exactly 1 yoctoNEAR".

// Zero deposit: what a function-call access key is limited to.
agent_cannot_call!(fc_key_cannot_set_agent, 0, "exactly 1 yoctoNEAR", |w| w
    .set_agent(
        agent(),
        U128(u128::MAX),
        U128(u128::MAX),
        vec![acc("attacker.near")]
    ));
agent_cannot_call!(fc_key_cannot_revoke_agent, 0, "exactly 1 yoctoNEAR", |w| w
    .revoke_agent(agent()));
agent_cannot_call!(fc_key_cannot_set_tokens, 0, "exactly 1 yoctoNEAR", |w| w
    .set_tokens(vec![dai()]));
agent_cannot_call!(fc_key_cannot_pause, 0, "exactly 1 yoctoNEAR", |w| w.pause());
agent_cannot_call!(fc_key_cannot_unpause, 0, "exactly 1 yoctoNEAR", |w| w
    .unpause());
agent_cannot_call!(fc_key_cannot_withdraw, 0, "exactly 1 yoctoNEAR", |w| w
    .withdraw(usdc(), U128(1), agent())
    .detach());
// One yocto attached (a full-access key on the agent account): the
// predecessor check refuses.
agent_cannot_call!(agent_full_key_cannot_set_agent, 1, "ERR_NOT_OWNER", |w| w
    .set_agent(agent(), U128(1), U128(1), vec![]));
agent_cannot_call!(agent_full_key_cannot_withdraw, 1, "ERR_NOT_OWNER", |w| w
    .withdraw(usdc(), U128(1), agent())
    .detach());
agent_cannot_call!(fc_key_cannot_set_review, 0, "exactly 1 yoctoNEAR", |w| w
    .set_review_policy(U128(1), true));
agent_cannot_call!(fc_key_cannot_approve, 0, "exactly 1 yoctoNEAR", |w| w
    .approve_payment(U64(0))
    .detach());
agent_cannot_call!(fc_key_cannot_reject, 0, "exactly 1 yoctoNEAR", |w| w
    .reject_payment(U64(0)));
agent_cannot_call!(fc_key_cannot_close, 0, "exactly 1 yoctoNEAR", |w| w
    .close(agent())
    .detach());
agent_cannot_call!(agent_full_key_cannot_close, 1, "ERR_NOT_OWNER", |w| w
    .close(agent())
    .detach());

#[test]
#[should_panic(expected = "Requires attached deposit of exactly 1 yoctoNEAR")]
fn owner_with_two_yocto_is_refused() {
    let mut w = setup();
    Ctx::new(owner()).deposit(2).set();
    w.pause();
}

// ----- set_agent / revoke_agent -----------------------------------------------

#[test]
fn set_agent_emits_and_normalizes_payees() {
    as_self().set();
    let mut w = PolicyWallet::new(owner(), vec![usdc()]);
    as_owner().set();
    w.set_agent(
        agent(),
        U128(PER_CALL),
        U128(DAILY),
        vec![gateway(), acc("b.near"), gateway()],
    );
    let view = w.get_agent(agent()).unwrap();
    assert_eq!(view.payees, vec![acc("b.near"), gateway()]);
    assert!(view.active);
    assert_eq!(
        events(),
        vec![
            r#"{"standard":"aps_policy_wallet","version":"1.0.0","event":"agent_set","data":{"agent_id":"agent.near","per_call_max":"50000","daily_max":"200000","payees":["b.near","gateway.near"]}}"#
        ]
    );
}

#[test]
fn set_agent_reconfigure_keeps_the_window() {
    let mut w = setup();
    as_agent().set();
    pay(&mut w, PER_CALL);
    as_owner().set();
    w.revoke_agent(agent());
    w.set_agent(agent(), U128(PER_CALL), U128(DAILY), vec![gateway()]);
    let view = w.get_agent(agent()).unwrap();
    assert!(view.active);
    assert_eq!(view.spent_in_window, U128(PER_CALL));
}

#[test]
#[should_panic(expected = "ERR_INVALID_POLICY")]
fn set_agent_rejects_zero_per_call() {
    let mut w = setup();
    as_owner().set();
    w.set_agent(agent(), U128(0), U128(DAILY), vec![gateway()]);
}

#[test]
#[should_panic(expected = "ERR_INVALID_POLICY")]
fn set_agent_rejects_daily_below_per_call() {
    let mut w = setup();
    as_owner().set();
    w.set_agent(agent(), U128(DAILY + 1), U128(DAILY), vec![gateway()]);
}

#[test]
#[should_panic(expected = "ERR_INVALID_POLICY")]
fn set_agent_rejects_too_many_payees() {
    let mut w = setup();
    as_owner().set();
    let payees = (0..=policy_core::MAX_PAYEES)
        .map(|i| acc(&format!("p{i}.near")))
        .collect();
    w.set_agent(agent(), U128(1), U128(1), payees);
}

#[test]
#[should_panic(expected = "ERR_SELF_ACCOUNT")]
fn set_agent_rejects_wallet_as_payee() {
    let mut w = setup();
    as_owner().set();
    w.set_agent(agent(), U128(1), U128(1), vec![wallet()]);
}

#[test]
fn revoke_agent_emits_and_deactivates() {
    let mut w = setup();
    as_owner().set();
    w.revoke_agent(agent());
    assert!(!w.get_agent(agent()).unwrap().active);
    assert_eq!(event_names(), vec!["agent_revoked"]);
}

#[test]
#[should_panic(expected = "ERR_AGENT_INACTIVE")]
fn revoke_unknown_agent() {
    let mut w = setup();
    as_owner().set();
    w.revoke_agent(acc("stranger.near"));
}

// ----- set_tokens / pause / unpause -------------------------------------------

#[test]
fn set_tokens_replaces_the_allowlist() {
    let mut w = setup();
    as_owner().set();
    w.set_tokens(vec![dai()]);
    assert_eq!(w.get_config().tokens, vec![dai()]);
    assert_eq!(event_names(), vec!["tokens_set"]);
}

#[test]
#[should_panic(expected = "ERR_TOKEN_NOT_ALLOWED")]
fn removed_token_is_refused() {
    let mut w = setup();
    as_owner().set();
    w.set_tokens(vec![dai()]);
    as_agent().set();
    pay(&mut w, 1);
}

#[test]
fn pause_and_unpause_toggle_and_emit() {
    let mut w = setup();
    as_owner().set();
    w.pause();
    assert!(w.get_config().paused);
    w.unpause();
    assert!(!w.get_config().paused);
    assert_eq!(event_names(), vec!["paused", "unpaused"]);
    as_agent().set();
    pay(&mut w, 1);
    assert_eq!(w.get_spent(agent()), U128(1));
}

// ----- withdraw -------------------------------------------------------------------

#[test]
fn withdraw_schedules_transfer_and_callback() {
    let mut w = setup();
    as_owner().set();
    w.pause();
    w.withdraw(dai(), U128(123), owner()).detach();
    let receipts = get_created_receipts();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].receiver_id, dai());
    let (method, deposit, args) = function_call(&receipts[0]);
    assert_eq!((method.as_str(), deposit), ("ft_transfer", 1));
    assert_eq!(
        args,
        r#"{"receiver_id":"owner.near","amount":"123","memo":null}"#
    );
    let (method, _, _) = function_call(&receipts[1]);
    assert_eq!(method, "on_withdraw_complete");
}

#[test]
#[should_panic(expected = "ERR_ZERO_AMOUNT")]
fn withdraw_zero_is_refused() {
    let mut w = setup();
    as_owner().set();
    w.withdraw(usdc(), U128(0), owner()).detach();
}

#[test]
#[should_panic(expected = "ERR_SELF_ACCOUNT")]
fn withdraw_to_self_is_refused() {
    let mut w = setup();
    as_owner().set();
    w.withdraw(usdc(), U128(1), wallet()).detach();
}

#[test]
fn withdraw_callback_reports_both_outcomes() {
    let mut w = setup();
    as_self().set_with_result(PromiseResult::Successful(vec![]));
    assert!(w.on_withdraw_complete(usdc(), U128(5), owner()));
    assert_eq!(event_names(), vec!["withdrawn"]);
    as_self().set_with_result(PromiseResult::Failed);
    assert!(!w.on_withdraw_complete(usdc(), U128(5), owner()));
    assert_eq!(event_names(), vec!["withdraw_failed"]);
}

// ----- ft_on_transfer ---------------------------------------------------------------

#[test]
fn ft_on_transfer_accepts_allowlisted_token() {
    let mut w = setup();
    Ctx::new(usdc()).set();
    match w.ft_on_transfer(owner(), U128(1_000_000), String::new()) {
        PromiseOrValue::Value(unused) => assert_eq!(unused, U128(0)),
        PromiseOrValue::Promise(_) => panic!("expected a value"),
    }
    assert_eq!(event_names(), vec!["funded"]);
}

#[test]
#[should_panic(expected = "ERR_TOKEN_NOT_ALLOWED")]
fn ft_on_transfer_refuses_other_tokens() {
    let mut w = setup();
    Ctx::new(dai()).set();
    let _ = w.ft_on_transfer(owner(), U128(1), String::new());
}

// ----- close ----------------------------------------------------------------------------

/// A paused wallet with its one token (USDC) allowlisted.
fn paused_wallet() -> PolicyWallet {
    let mut w = setup();
    as_owner().set();
    w.pause();
    w
}

fn delete_beneficiaries() -> Vec<AccountId> {
    get_created_receipts()
        .iter()
        .flat_map(|r| r.actions.iter())
        .filter_map(|a| match a {
            near_sdk::mock::MockAction::DeleteAccount { beneficiary_id, .. } => {
                Some(beneficiary_id.clone())
            }
            _ => None,
        })
        .collect()
}

fn balance(b: u128) -> PromiseResult {
    PromiseResult::Successful(format!("\"{b}\"").into_bytes())
}

fn close_check(w: &mut PolicyWallet, results: Vec<PromiseResult>) -> Option<bool> {
    as_self().set_with_results(results);
    match w.on_close_check(owner(), vec![usdc()]) {
        PromiseOrValue::Value(v) => Some(v),
        PromiseOrValue::Promise(p) => {
            p.detach();
            None
        }
    }
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn close_requires_one_yocto_from_owner() {
    let mut w = paused_wallet();
    Ctx::new(owner()).set();
    w.close(owner()).detach();
}

#[test]
#[should_panic(expected = "ERR_NOT_PAUSED")]
fn close_requires_paused() {
    let mut w = setup();
    as_owner().set();
    w.close(owner()).detach();
}

#[test]
#[should_panic(expected = "ERR_SELF_ACCOUNT")]
fn close_rejects_wallet_as_beneficiary() {
    let mut w = paused_wallet();
    as_owner().set();
    w.close(wallet()).detach();
}

#[test]
#[should_panic(expected = "ERR_INSUFFICIENT_GAS")]
fn close_requires_gas_for_the_query_and_the_callback() {
    // 5 TGas for the balance query, 15 for the callback, 10 of headroom.
    let mut w = paused_wallet();
    as_owner().gas(29).set();
    w.close(owner()).detach();
}

#[test]
fn close_queries_the_allowlisted_token_then_checks() {
    let mut w = paused_wallet();
    as_owner().set();
    w.close(owner()).detach();
    let receipts = get_created_receipts();
    let calls: Vec<(AccountId, String)> = receipts
        .iter()
        .map(|r| (r.receiver_id.clone(), function_call(r).0))
        .collect();
    assert_eq!(
        calls,
        vec![
            (usdc(), "ft_balance_of".to_string()),
            (wallet(), "on_close_check".to_string()),
        ]
    );
    let (_, deposit, args) = function_call(&receipts[0]);
    assert_eq!(deposit, 0);
    assert_eq!(args, r#"{"account_id":"agent-wallet.owner.near"}"#);
    assert!(delete_beneficiaries().is_empty());
}

#[test]
fn close_check_with_all_zero_balances_deletes_the_account() {
    let mut w = paused_wallet();
    assert_eq!(close_check(&mut w, vec![balance(0)]), None);
    assert_eq!(event_names(), vec!["closed"]);
    assert_eq!(
        events()[0],
        r#"{"standard":"aps_policy_wallet","version":"1.0.0","event":"closed","data":{"beneficiary":"owner.near"}}"#
    );
    assert_eq!(delete_beneficiaries(), vec![owner()]);
}

#[test]
fn close_check_with_a_nonzero_balance_refuses() {
    let mut w = paused_wallet();
    assert_eq!(close_check(&mut w, vec![balance(7)]), Some(false));
    assert_eq!(
        events(),
        vec![
            r#"{"standard":"aps_policy_wallet","version":"1.0.0","event":"close_refused","data":{"tokens":[{"token":"usdc.near","balance":"7"}]}}"#
        ]
    );
    assert!(delete_beneficiaries().is_empty());
    assert!(w.get_config().paused);
}

#[test]
fn close_check_with_a_failed_query_refuses() {
    let mut w = paused_wallet();
    assert_eq!(
        close_check(&mut w, vec![PromiseResult::Failed]),
        Some(false)
    );
    assert!(events()[0].contains(r#"{"token":"usdc.near","balance":null}"#));
    assert!(delete_beneficiaries().is_empty());
}

#[test]
fn close_check_with_an_unreadable_balance_refuses() {
    let mut w = paused_wallet();
    let garbage = PromiseResult::Successful(b"\"not a number\"".to_vec());
    assert_eq!(close_check(&mut w, vec![garbage]), Some(false));
    assert_eq!(event_names(), vec!["close_refused"]);
    assert!(delete_beneficiaries().is_empty());
}

#[test]
fn close_with_no_tokens_deletes_at_once() {
    let mut w = setup();
    as_owner().set();
    w.set_tokens(vec![]);
    w.pause();
    match w.close(owner()) {
        PromiseOrValue::Promise(p) => p.detach(),
        PromiseOrValue::Value(_) => panic!("expected the delete promise"),
    }
    assert_eq!(event_names(), vec!["tokens_set", "paused", "closed"]);
    assert_eq!(delete_beneficiaries(), vec![owner()]);
}

// ----- views --------------------------------------------------------------------------

#[test]
fn views_for_unknown_agent() {
    let w = setup();
    as_agent().set();
    assert_eq!(w.get_agent(acc("stranger.near")), None);
    assert_eq!(w.get_spent(acc("stranger.near")), U128(0));
    let review = w.get_review_policy();
    assert_eq!(review.approval_threshold, U128(0));
    assert!(!review.review_unknown_payees);
    assert!(w.get_pending().is_empty());
}

fn enable_review(w: &mut PolicyWallet, threshold: u128) {
    as_owner().set();
    w.set_review_policy(U128(threshold), true);
}

#[test]
fn review_defaults_off_and_an_unknown_payee_is_still_refused() {
    let mut w = setup();
    as_agent().set();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settle(w.pay(usdc(), acc("stranger.near"), U128(1), None));
    }));
    assert!(caught.is_err());
    assert!(w.get_pending().is_empty());
    assert_eq!(w.get_spent(agent()), U128(0));
}

#[test]
fn unknown_payee_is_queued_when_review_is_on_and_counts_against_the_day() {
    let mut w = setup();
    enable_review(&mut w, 0);
    as_agent().set();
    let stranger = acc("stranger.near");
    match w.pay(usdc(), stranger.clone(), U128(1_000), Some("hold".into())) {
        PromiseOrValue::Value(U64(id)) => assert_eq!(id, 0),
        PromiseOrValue::Promise(_) => panic!("a new payee must wait, not transfer"),
    }
    assert!(get_created_receipts().is_empty());
    let pending = w.get_pending();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].reason, REASON_NEW_PAYEE);
    assert_eq!(pending[0].receiver_id, stranger);
    assert_eq!(pending[0].memo.as_deref(), Some("hold"));
    assert_eq!(w.get_spent(agent()), U128(1_000));
    assert!(events()[0].contains(r#""event":"payment_queued""#));
    assert!(events()[0].contains(r#""reason":"new_payee""#));
}

#[test]
fn allowlisted_amount_above_the_threshold_waits_for_the_owner() {
    let mut w = setup();
    enable_review(&mut w, 1_000);
    as_agent().set();
    // At the threshold the payment still settles.
    settle(w.pay(usdc(), gateway(), U128(1_000), None));
    assert!(w.get_pending().is_empty());
    match w.pay(usdc(), gateway(), U128(1_001), None) {
        PromiseOrValue::Value(U64(id)) => assert_eq!(id, 1),
        PromiseOrValue::Promise(_) => panic!("above the threshold must wait"),
    }
    assert_eq!(w.get_pending()[0].reason, REASON_ABOVE_THRESHOLD);
    assert_eq!(w.get_spent(agent()), U128(2_001));
}

#[test]
fn queued_payment_still_obeys_the_per_call_and_daily_limits() {
    let mut w = setup();
    enable_review(&mut w, 0);
    as_agent().set();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settle(w.pay(usdc(), acc("stranger.near"), U128(PER_CALL + 1), None));
    }));
    assert!(caught.is_err());
    assert!(w.get_pending().is_empty());
    assert_eq!(w.get_spent(agent()), U128(0));
}

#[test]
fn owner_rejects_a_queued_payment_and_restores_the_budget() {
    let mut w = setup();
    enable_review(&mut w, 0);
    as_agent().set();
    settle(w.pay(usdc(), acc("stranger.near"), U128(4_000), None));
    as_owner().set();
    w.reject_payment(U64(0));
    assert!(w.get_pending().is_empty());
    assert_eq!(w.get_spent(agent()), U128(0));
    assert!(events()
        .iter()
        .any(|event| event.contains(r#""event":"payment_rejected""#)
            && event.contains(r#""release":"restored""#)));
}

#[test]
fn owner_approves_a_queued_payment_and_schedules_the_transfer() {
    let mut w = setup();
    enable_review(&mut w, 0);
    as_agent().set();
    settle(w.pay(
        usdc(),
        acc("stranger.near"),
        U128(4_000),
        Some("later".into()),
    ));
    as_owner().set();
    w.approve_payment(U64(0)).detach();
    assert!(w.get_pending().is_empty());
    // The reservation stays until the transfer callback.
    assert_eq!(w.get_spent(agent()), U128(4_000));
    let receipts = get_created_receipts();
    assert_eq!(receipts.len(), 2);
    let (method, deposit, args) = function_call(&receipts[0]);
    assert_eq!((method.as_str(), deposit), ("ft_transfer", 1));
    assert_eq!(
        args,
        r#"{"receiver_id":"stranger.near","amount":"4000","memo":"later"}"#
    );
    assert!(events()
        .iter()
        .any(|event| event.contains(r#""event":"pay_requested""#)));
}

#[test]
#[should_panic(expected = "ERR_NOT_PENDING")]
fn approve_missing_payment_panics() {
    let mut w = setup();
    as_owner().set();
    w.approve_payment(U64(9)).detach();
}

#[test]
fn approve_refuses_a_revoked_agent_and_keeps_the_queue() {
    let mut w = setup();
    enable_review(&mut w, 0);
    as_agent().set();
    settle(w.pay(usdc(), acc("stranger.near"), U128(10), None));
    as_owner().set();
    w.revoke_agent(agent());
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        w.approve_payment(U64(0)).detach();
    }));
    assert!(caught.is_err());
    assert_eq!(w.get_pending().len(), 1);
    assert_eq!(w.get_spent(agent()), U128(10));
}

#[test]
fn reject_still_works_after_revoke() {
    let mut w = setup();
    enable_review(&mut w, 0);
    as_agent().set();
    settle(w.pay(usdc(), acc("stranger.near"), U128(10), None));
    as_owner().set();
    w.revoke_agent(agent());
    w.reject_payment(U64(0));
    assert!(w.get_pending().is_empty());
    assert_eq!(w.get_spent(agent()), U128(0));
}

#[test]
#[should_panic(expected = "ERR_REVIEW_QUEUE_FULL")]
fn the_queue_stops_at_eight_and_does_not_reserve_the_ninth() {
    let mut w = setup();
    enable_review(&mut w, 0);
    as_agent().set();
    for index in 0..MAX_PENDING {
        let payee = acc(&format!("p{index}.near"));
        settle(w.pay(usdc(), payee, U128(1), None));
    }
    assert_eq!(w.get_pending().len(), MAX_PENDING);
    assert_eq!(w.get_spent(agent()), U128(MAX_PENDING as u128));
    settle(w.pay(usdc(), acc("overflow.near"), U128(1), None));
}
