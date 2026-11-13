//! Unit tests over near-sdk's mocked blockchain.
//!
//! Sandbox tests cover a real account being created and a real refund.
//! These tests cover every method, every error code, and the receipts the
//! factory schedules.

use super::*;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::{testing_env, PromiseResult, RuntimeFeesConfig};

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}
fn factory() -> AccountId {
    acc("triagepay.hlpok1lad.tg")
}
fn owner() -> AccountId {
    acc("hlpok1lad.tg")
}
fn user() -> AccountId {
    acc("alice.near")
}
fn stranger() -> AccountId {
    acc("mallory.near")
}

struct Ctx {
    predecessor: AccountId,
    deposit: u128,
    gas_tgas: u64,
}

impl Ctx {
    fn new(predecessor: AccountId) -> Self {
        Self {
            predecessor,
            deposit: 0,
            gas_tgas: 300,
        }
    }
    fn deposit(mut self, yocto: u128) -> Self {
        self.deposit = yocto;
        self
    }
    fn gas(mut self, tgas: u64) -> Self {
        self.gas_tgas = tgas;
        self
    }
    fn set(self) {
        let context = VMContextBuilder::new()
            .current_account_id(factory())
            .signer_account_id(self.predecessor.clone())
            .predecessor_account_id(self.predecessor)
            .attached_deposit(NearToken::from_yoctonear(self.deposit))
            .prepaid_gas(Gas::from_tgas(self.gas_tgas))
            .build();
        testing_env!(context);
    }
    fn set_with_result(self, result: PromiseResult) {
        let context = VMContextBuilder::new()
            .current_account_id(factory())
            .signer_account_id(self.predecessor.clone())
            .predecessor_account_id(self.predecessor.clone())
            .attached_deposit(NearToken::from_yoctonear(self.deposit))
            .prepaid_gas(Gas::from_tgas(self.gas_tgas))
            .build();
        testing_env!(
            context,
            near_sdk::test_vm_config(),
            RuntimeFeesConfig::test(),
            Default::default(),
            vec![result]
        );
    }
}

fn as_factory() -> Ctx {
    Ctx::new(factory())
}
fn as_owner() -> Ctx {
    Ctx::new(owner()).deposit(1)
}
fn as_user() -> Ctx {
    Ctx::new(user())
}

fn setup() -> WalletFactory {
    as_factory().set();
    WalletFactory::new(owner(), owner(), vec![acc(USDC_MAINNET), acc(WRAP_NEAR)])
}

fn cost() -> u128 {
    SERVICE_FEE + WALLET_STORAGE_BYTES as u128 * STORAGE_BYTE_COST
}

fn events() -> Vec<String> {
    get_logs()
        .into_iter()
        .filter_map(|l| l.strip_prefix("EVENT_JSON:").map(str::to_string))
        .collect()
}

fn ready() -> WalletFactory {
    let mut f = setup();
    as_owner().set();
    f.update_wallet_code_hash("ab".repeat(32));
    f
}

#[test]
fn new_starts_with_one_near_and_the_default_recipient() {
    let f = setup();
    let view = f.get_config();
    assert_eq!(view.owner, owner());
    assert_eq!(view.fee, U128(SERVICE_FEE));
    assert_eq!(view.fee_recipient, owner());
    assert!(!view.paused);
    assert_eq!(view.wallets_created, 0);
    assert!(view.code_hash_hex.is_none());
    assert_eq!(view.default_tokens, vec![acc(USDC_MAINNET), acc(WRAP_NEAR)]);
    let quote = f.get_create_cost();
    assert_eq!(quote.fee, U128(SERVICE_FEE));
    assert_eq!(quote.total.0, quote.fee.0 + quote.storage.0);
    assert_eq!(
        quote.storage.0,
        WALLET_STORAGE_BYTES as u128 * STORAGE_BYTE_COST
    );
    assert_eq!(
        f.get_wallet_id("alpha".into()).as_deref(),
        Some("alpha.triagepay.hlpok1lad.tg")
    );
}

#[test]
#[should_panic(expected = "at most 8 default tokens")]
fn new_rejects_too_many_default_tokens() {
    as_factory().set();
    let tokens: Vec<AccountId> = (0..9).map(|i| acc(&format!("t{i}.near"))).collect();
    WalletFactory::new(owner(), owner(), tokens);
}

#[test]
#[should_panic(expected = "ERR_CODE_UNSET")]
fn create_before_publish_is_refused() {
    let mut f = setup();
    as_user().deposit(cost()).set();
    f.create_wallet("alpha".into()).detach();
}

#[test]
#[should_panic(expected = "ERR_PAUSED")]
fn create_while_paused_is_refused() {
    let mut f = ready();
    as_owner().set();
    f.pause_creation();
    as_user().deposit(cost()).set();
    f.create_wallet("alpha".into()).detach();
}

#[test]
#[should_panic(expected = "ERR_INSUFFICIENT_GAS")]
fn create_with_too_little_gas_is_refused() {
    let mut f = ready();
    as_user().deposit(cost()).gas(10).set();
    f.create_wallet("alpha".into()).detach();
}

#[test]
#[should_panic(expected = "ERR_BAD_NAME")]
fn create_rejects_a_short_name() {
    let mut f = ready();
    as_user().deposit(cost()).set();
    f.create_wallet("a".into()).detach();
}

#[test]
#[should_panic(expected = "ERR_BAD_NAME")]
fn create_rejects_uppercase_and_dots() {
    let mut f = ready();
    as_user().deposit(cost()).set();
    f.create_wallet("Bad.Name".into()).detach();
}

#[test]
#[should_panic(expected = "ERR_BAD_NAME")]
fn create_rejects_a_separator_at_the_edge() {
    let mut f = ready();
    as_user().deposit(cost()).set();
    f.create_wallet("-ab".into()).detach();
}

#[test]
#[should_panic(expected = "ERR_INSUFFICIENT_DEPOSIT")]
fn create_rejects_a_short_deposit_and_does_not_count_a_wallet() {
    let mut f = ready();
    as_user().deposit(cost() - 1).set();
    f.create_wallet("alpha".into()).detach();
}

#[test]
fn a_short_deposit_panic_leaves_the_counter_at_zero() {
    let mut f = ready();
    as_user().deposit(cost() - 1).set();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f.create_wallet("alpha".into()).detach();
    }));
    assert!(caught.is_err());
    assert_eq!(f.get_config().wallets_created, 0);
}

#[test]
fn exact_deposit_schedules_creation_and_does_not_count_yet() {
    let mut f = ready();
    as_user().deposit(cost()).set();
    f.create_wallet("alpha".into()).detach();
    assert_eq!(f.get_config().wallets_created, 0);
    let receipts = get_created_receipts();
    assert!(
        receipts
            .iter()
            .any(|r| r.receiver_id.as_str() == "alpha.triagepay.hlpok1lad.tg"),
        "{:?}",
        receipts
            .iter()
            .map(|r| r.receiver_id.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn excess_is_scheduled_back_to_the_user() {
    let mut f = ready();
    let extra = 2 * SERVICE_FEE;
    as_user().deposit(cost() + extra).set();
    f.create_wallet("alpha".into()).detach();
    let receipts = get_created_receipts();
    let refund = receipts.iter().find(|r| r.receiver_id == user());
    let refund = refund.expect("excess refund receipt");
    let transferred: u128 = refund
        .actions
        .iter()
        .filter_map(|action| match action {
            near_sdk::mock::MockAction::Transfer { deposit, .. } => Some(deposit.as_yoctonear()),
            _ => None,
        })
        .sum();
    assert_eq!(transferred, extra);
}

#[test]
fn failed_creation_refunds_fee_and_storage_and_does_not_pay_the_recipient() {
    let mut f = ready();
    as_factory().set_with_result(PromiseResult::Failed);
    f.on_wallet_created(
        user(),
        acc("alpha.triagepay.hlpok1lad.tg"),
        U128(SERVICE_FEE),
        U128(storage_cost()),
    );
    assert_eq!(f.get_config().wallets_created, 0);
    let receipts = get_created_receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].receiver_id, user());
    let transferred: u128 = receipts[0]
        .actions
        .iter()
        .filter_map(|action| match action {
            near_sdk::mock::MockAction::Transfer { deposit, .. } => Some(deposit.as_yoctonear()),
            _ => None,
        })
        .sum();
    assert_eq!(transferred, SERVICE_FEE + storage_cost());
    assert!(events()[0].contains(r#""event":"wallet_creation_failed""#));
    assert!(events()[0].contains(&format!(
        "\"refunded\":\"{}\"",
        SERVICE_FEE + storage_cost()
    )));
}

#[test]
fn successful_creation_pays_the_fee_recipient_and_emits() {
    let mut f = ready();
    as_factory().set_with_result(PromiseResult::Successful(Vec::new()));
    f.on_wallet_created(
        user(),
        acc("alpha.triagepay.hlpok1lad.tg"),
        U128(SERVICE_FEE),
        U128(storage_cost()),
    );
    assert_eq!(f.get_config().wallets_created, 1);
    let receipts = get_created_receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].receiver_id, owner());
    let transferred: u128 = receipts[0]
        .actions
        .iter()
        .filter_map(|action| match action {
            near_sdk::mock::MockAction::Transfer { deposit, .. } => Some(deposit.as_yoctonear()),
            _ => None,
        })
        .sum();
    assert_eq!(transferred, SERVICE_FEE);
    let logged = events();
    assert!(logged[0].contains(r#""standard":"triagepay_factory""#));
    assert!(logged[0].contains(r#""event":"wallet_created""#));
    assert!(logged[0].contains(r#""wallet_id":"alpha.triagepay.hlpok1lad.tg""#));
    assert!(logged[0].contains(r#""owner":"alice.near""#));
}

#[test]
fn a_zero_fee_success_does_not_transfer() {
    let mut f = ready();
    as_owner().set();
    f.set_fee(U128(0));
    as_factory().set_with_result(PromiseResult::Successful(Vec::new()));
    f.on_wallet_created(
        user(),
        acc("alpha.triagepay.hlpok1lad.tg"),
        U128(0),
        U128(storage_cost()),
    );
    assert_eq!(f.get_config().wallets_created, 1);
    assert!(get_created_receipts().is_empty());
}

#[test]
fn publish_failure_does_not_store_the_hash() {
    let mut f = setup();
    as_factory().set_with_result(PromiseResult::Failed);
    f.on_code_published(vec![7u8; 32]);
    assert!(f.get_config().code_hash_hex.is_none());
}

#[test]
fn publish_success_stores_the_hash() {
    let mut f = setup();
    as_factory().set_with_result(PromiseResult::Successful(Vec::new()));
    let hash = vec![0xabu8; 32];
    f.on_code_published(hash.clone());
    assert_eq!(f.get_config().code_hash_hex.unwrap(), "ab".repeat(32));
}

#[test]
fn publish_ignores_a_hash_of_the_wrong_length() {
    let mut f = setup();
    as_factory().set_with_result(PromiseResult::Successful(Vec::new()));
    f.on_code_published(vec![1, 2, 3]);
    assert!(f.get_config().code_hash_hex.is_none());
}

#[test]
#[should_panic(expected = "ERR_BAD_CODE")]
fn publish_rejects_empty_code() {
    let mut f = setup();
    as_owner().set();
    f.publish_wallet_code(Base64VecU8(Vec::new())).detach();
}

#[test]
#[should_panic(expected = "ERR_NOT_OWNER")]
fn publish_refuses_a_non_owner() {
    let mut f = setup();
    Ctx::new(stranger()).deposit(1).set();
    f.publish_wallet_code(Base64VecU8(vec![1, 2, 3])).detach();
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn publish_requires_one_yocto() {
    let mut f = setup();
    Ctx::new(owner()).set();
    f.publish_wallet_code(Base64VecU8(vec![1])).detach();
}

#[test]
#[should_panic(expected = "ERR_BAD_HASH")]
fn update_hash_rejects_odd_hex() {
    let mut f = setup();
    as_owner().set();
    f.update_wallet_code_hash("abc".into());
}

#[test]
#[should_panic(expected = "ERR_BAD_HASH")]
fn update_hash_rejects_non_hex() {
    let mut f = setup();
    as_owner().set();
    f.update_wallet_code_hash("zz".repeat(32));
}

#[test]
#[should_panic(expected = "ERR_NOT_OWNER")]
fn update_hash_refuses_a_non_owner() {
    let mut f = setup();
    Ctx::new(stranger()).deposit(1).set();
    f.update_wallet_code_hash("ab".repeat(32));
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn update_hash_requires_one_yocto() {
    let mut f = setup();
    Ctx::new(owner()).set();
    f.update_wallet_code_hash("ab".repeat(32));
}

#[test]
fn set_fee_and_recipient_round_trip() {
    let mut f = setup();
    as_owner().set();
    f.set_fee(U128(5));
    f.set_fee_recipient(user());
    let view = f.get_config();
    assert_eq!(view.fee, U128(5));
    assert_eq!(view.fee_recipient, user());
    let quote = f.get_create_cost();
    assert_eq!(quote.fee, U128(5));
    assert_eq!(quote.fee_recipient, user());
    assert_eq!(quote.total.0, 5 + quote.storage.0);
}

#[test]
#[should_panic(expected = "ERR_NOT_OWNER")]
fn set_fee_refuses_a_non_owner() {
    let mut f = setup();
    Ctx::new(stranger()).deposit(1).set();
    f.set_fee(U128(1));
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn set_fee_requires_one_yocto() {
    let mut f = setup();
    Ctx::new(owner()).set();
    f.set_fee(U128(1));
}

#[test]
#[should_panic(expected = "ERR_NOT_OWNER")]
fn set_fee_recipient_refuses_a_non_owner() {
    let mut f = setup();
    Ctx::new(stranger()).deposit(1).set();
    f.set_fee_recipient(user());
}

#[test]
#[should_panic(expected = "ERR_BAD_FEE")]
fn set_fee_recipient_refuses_the_factory_itself() {
    let mut f = setup();
    as_owner().set();
    f.set_fee_recipient(factory());
}

#[test]
fn pause_and_resume() {
    let mut f = setup();
    as_owner().set();
    f.pause_creation();
    assert!(f.get_config().paused);
    assert!(f.get_create_cost().paused);
    f.resume_creation();
    assert!(!f.get_config().paused);
}

#[test]
#[should_panic(expected = "ERR_NOT_OWNER")]
fn pause_refuses_a_non_owner() {
    let mut f = setup();
    Ctx::new(stranger()).deposit(1).set();
    f.pause_creation();
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn resume_requires_one_yocto() {
    let mut f = setup();
    Ctx::new(owner()).set();
    f.resume_creation();
}

#[test]
fn get_wallet_id_rejects_garbage() {
    let f = setup();
    assert_eq!(f.get_wallet_id("".into()), None);
    assert_eq!(f.get_wallet_id("A".into()), None);
    assert_eq!(f.get_wallet_id("has space".into()), None);
    assert_eq!(
        f.get_wallet_id("agent_1".into()).as_deref(),
        Some("agent_1.triagepay.hlpok1lad.tg")
    );
}

#[test]
fn two_success_callbacks_count_two_wallets() {
    let mut f = ready();
    as_factory().set_with_result(PromiseResult::Successful(Vec::new()));
    f.on_wallet_created(
        user(),
        acc("a.triagepay.hlpok1lad.tg"),
        U128(SERVICE_FEE),
        U128(1),
    );
    as_factory().set_with_result(PromiseResult::Successful(Vec::new()));
    f.on_wallet_created(
        stranger(),
        acc("b.triagepay.hlpok1lad.tg"),
        U128(SERVICE_FEE),
        U128(1),
    );
    assert_eq!(f.get_config().wallets_created, 2);
}
