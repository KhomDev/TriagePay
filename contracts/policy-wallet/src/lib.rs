//! Spend-policy wallet contract.
//!
//! The wallet holds NEP-141 balances on behalf of its owner. Registered
//! agent accounts call `pay` with zero deposit (a function-call access key
//! is enough); the wallet checks the agent's policy through `policy-core`,
//! reserves the amount, and performs the `ft_transfer` with 1 yoctoNEAR from
//! its own balance. A failed transfer releases the reservation in the
//! `on_pay_complete` callback.
//!
//! Every owner method requires exactly 1 yoctoNEAR attached and the owner
//! as predecessor. Function-call access keys cannot attach a deposit, so no
//! agent key can reach an owner method even when the key's method list
//! would allow it.
//!
//! The contract has no upgrade method. Redeploying to a new account is the
//! upgrade path.

pub mod events;

#[cfg(test)]
mod tests;

use events::{ReleaseStatus, WalletEvent};
use near_sdk::json_types::{U128, U64};
use near_sdk::store::LookupMap;
use near_sdk::{
    assert_one_yocto, env, ext_contract, near, AccountId, BorshStorageKey, Gas, NearToken,
    PanicOnDefault, Promise, PromiseError, PromiseOrValue,
};
use policy_core::{
    next_window_start, window_index, AgentPolicy, PolicyError, ReleaseOutcome, Reservation,
    WalletGate,
};

/// The caller is not the wallet owner.
pub const ERR_NOT_OWNER: &str = "ERR_NOT_OWNER";
/// `pay` was called with a non-zero deposit.
pub const ERR_DEPOSIT_NOT_ZERO: &str = "ERR_DEPOSIT_NOT_ZERO";
/// `pay` was called with less gas than the transfer and callback need.
pub const ERR_INSUFFICIENT_GAS: &str = "ERR_INSUFFICIENT_GAS";
/// The memo is longer than [`MAX_MEMO_LEN`] bytes.
pub const ERR_MEMO_TOO_LONG: &str = "ERR_MEMO_TOO_LONG";
/// More than [`MAX_TOKENS`] tokens were allowlisted.
pub const ERR_TOO_MANY_TOKENS: &str = "ERR_TOO_MANY_TOKENS";
/// The wallet's own account was used as an agent, payee or destination.
pub const ERR_SELF_ACCOUNT: &str = "ERR_SELF_ACCOUNT";

/// Largest accepted memo, in bytes.
pub const MAX_MEMO_LEN: usize = 256;
/// Largest token allowlist.
pub const MAX_TOKENS: usize = 16;

/// Gas attached to each `ft_transfer`.
pub const GAS_FT_TRANSFER: Gas = Gas::from_tgas(15);
/// Gas attached to each completion callback.
pub const GAS_CALLBACK: Gas = Gas::from_tgas(10);
/// Minimum prepaid gas for `pay` and `withdraw`: the transfer, the callback
/// and headroom for the calling method itself. Below this the callback
/// could run out of gas and leave a reservation unreleased.
pub const GAS_MIN_PREPAID: Gas = Gas::from_tgas(35);

/// One yoctoNEAR, the deposit NEP-141 `ft_transfer` requires.
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

/// The NEP-141 methods the wallet calls on token contracts.
#[ext_contract(ext_ft)]
pub trait FungibleToken {
    /// NEP-141 transfer; requires exactly 1 yoctoNEAR attached.
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

#[near]
#[derive(BorshStorageKey)]
enum StorageKey {
    Agents,
}

/// An agent's stored policy. Mirrors [`policy_core::AgentPolicy`] with
/// account ids instead of strings; every change goes through the core.
#[near(serializers = [borsh])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgent {
    per_call_max: u128,
    daily_max: u128,
    payees: Vec<AccountId>,
    active: bool,
    spent_in_window: u128,
    window_index: u64,
}

impl StoredAgent {
    fn policy(&self) -> AgentPolicy {
        AgentPolicy::from_parts(
            self.per_call_max,
            self.daily_max,
            self.payees.iter().map(|p| p.to_string()).collect(),
            self.active,
            self.spent_in_window,
            self.window_index,
        )
        .unwrap_or_else(|e| fail(e))
    }

    fn store_spend(&mut self, policy: &AgentPolicy) {
        self.active = policy.active();
        self.spent_in_window = policy.spent_in_window();
        self.window_index = policy.window_index();
    }
}

/// View of an agent's policy as of the current block.
#[near(serializers = [json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentView {
    /// Largest single payment.
    pub per_call_max: U128,
    /// Largest total per UTC day.
    pub daily_max: U128,
    /// Payee allowlist, sorted.
    pub payees: Vec<AccountId>,
    /// False once revoked.
    pub active: bool,
    /// Spent in the current UTC day (zero after rollover).
    pub spent_in_window: U128,
    /// Remaining budget in the current UTC day.
    pub remaining: U128,
    /// Current UTC day number.
    pub window_index: U64,
    /// Block timestamp (ns) at which the window next resets.
    pub resets_at_ns: U64,
}

/// View of the wallet configuration.
#[near(serializers = [json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigView {
    /// Owner account.
    pub owner: AccountId,
    /// True while paused.
    pub paused: bool,
    /// Token allowlist, sorted.
    pub tokens: Vec<AccountId>,
    /// Id the next payment will receive.
    pub next_payment_id: U64,
    /// NEP-297 event standard name.
    pub event_standard: String,
    /// NEP-297 event version.
    pub event_version: String,
}

/// The spend-policy wallet.
#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct PolicyWallet {
    owner: AccountId,
    paused: bool,
    tokens: Vec<AccountId>,
    agents: LookupMap<AccountId, StoredAgent>,
    next_payment_id: u64,
}

fn fail(err: PolicyError) -> ! {
    env::panic_str(&err.to_string())
}

fn fail_with(code: &str, description: &str) -> ! {
    env::panic_str(&format!("{code}: {description}"))
}

fn normalize_tokens(mut tokens: Vec<AccountId>) -> Vec<AccountId> {
    tokens.sort();
    tokens.dedup();
    if tokens.len() > MAX_TOKENS {
        fail_with(ERR_TOO_MANY_TOKENS, "token allowlist is too long");
    }
    if tokens.contains(&env::current_account_id()) {
        fail_with(ERR_SELF_ACCOUNT, "the wallet cannot be its own token");
    }
    tokens
}

#[near]
impl PolicyWallet {
    /// Initialize the wallet. Callable once, and only by the wallet account
    /// itself (deploy and `new` go in one batch transaction).
    #[init]
    #[private]
    pub fn new(owner: AccountId, tokens: Vec<AccountId>) -> Self {
        if owner == env::current_account_id() {
            fail_with(ERR_SELF_ACCOUNT, "the wallet cannot own itself");
        }
        Self {
            owner,
            paused: false,
            tokens: normalize_tokens(tokens),
            agents: LookupMap::new(StorageKey::Agents),
            next_payment_id: 0,
        }
    }

    // ----- agent method ---------------------------------------------------

    /// Pay `amount` of `token` to `receiver_id` on behalf of the calling
    /// agent. Zero deposit; at least [`GAS_MIN_PREPAID`] gas.
    ///
    /// Refusals panic with a stable code and change no state: `ERR_PAUSED`,
    /// `ERR_AGENT_INACTIVE`, `ERR_TOKEN_NOT_ALLOWED`, `ERR_PAYEE_NOT_ALLOWED`,
    /// `ERR_ZERO_AMOUNT`, `ERR_PER_CALL_LIMIT`, `ERR_DAILY_LIMIT`,
    /// `ERR_OVERFLOW`, plus `ERR_DEPOSIT_NOT_ZERO`, `ERR_MEMO_TOO_LONG` and
    /// `ERR_INSUFFICIENT_GAS`.
    pub fn pay(
        &mut self,
        token: AccountId,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
    ) -> Promise {
        if !env::attached_deposit().is_zero() {
            fail_with(ERR_DEPOSIT_NOT_ZERO, "pay takes no deposit");
        }
        if memo.as_ref().is_some_and(|m| m.len() > MAX_MEMO_LEN) {
            fail_with(ERR_MEMO_TOO_LONG, "memo exceeds 256 bytes");
        }
        if env::prepaid_gas() < GAS_MIN_PREPAID {
            fail_with(ERR_INSUFFICIENT_GAS, "attach at least 35 TGas");
        }
        if self.paused {
            fail(PolicyError::Paused);
        }
        let agent_id = env::predecessor_account_id();
        let gate = WalletGate {
            paused: self.paused,
            token_allowed: self.tokens.contains(&token),
        };
        let now = env::block_timestamp();
        let stored = self
            .agents
            .get_mut(&agent_id)
            .unwrap_or_else(|| fail(PolicyError::AgentInactive));
        let mut policy = stored.policy();
        let reservation = policy
            .check_and_reserve(gate, receiver_id.as_str(), amount.0, now)
            .unwrap_or_else(|e| fail(e));
        stored.store_spend(&policy);

        let payment_id = self.next_payment_id;
        self.next_payment_id = payment_id
            .checked_add(1)
            .unwrap_or_else(|| fail(PolicyError::Overflow));

        WalletEvent::PayRequested {
            payment_id: U64(payment_id),
            agent_id: agent_id.clone(),
            token: token.clone(),
            receiver_id: receiver_id.clone(),
            amount,
            window_index: U64(reservation.window_index),
        }
        .emit();

        ext_ft::ext(token.clone())
            .with_attached_deposit(ONE_YOCTO)
            .with_static_gas(GAS_FT_TRANSFER)
            .ft_transfer(receiver_id.clone(), amount, memo)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_pay_complete(
                        agent_id,
                        token,
                        receiver_id,
                        amount,
                        U64(payment_id),
                        U64(reservation.window_index),
                    ),
            )
    }

    /// Completion callback for `pay`. Emits `pay_settled` on success; on
    /// failure releases the reservation and emits `pay_failed`. Never
    /// panics, so its state change is never reverted. Returns true when the
    /// transfer settled.
    #[private]
    pub fn on_pay_complete(
        &mut self,
        agent_id: AccountId,
        token: AccountId,
        receiver_id: AccountId,
        amount: U128,
        payment_id: U64,
        window_index: U64,
    ) -> bool {
        if transfer_succeeded() {
            WalletEvent::PaySettled {
                payment_id,
                agent_id,
                token,
                receiver_id,
                amount,
            }
            .emit();
            return true;
        }
        let release = match self.agents.get_mut(&agent_id) {
            Some(stored) => {
                let mut policy = stored.policy();
                let reservation = Reservation {
                    amount: amount.0,
                    window_index: window_index.0,
                };
                match policy.release(reservation) {
                    Ok(ReleaseOutcome::Restored) => {
                        stored.store_spend(&policy);
                        ReleaseStatus::Restored
                    }
                    Ok(ReleaseOutcome::WindowExpired) => ReleaseStatus::WindowExpired,
                    Err(_) => ReleaseStatus::Mismatch,
                }
            }
            None => ReleaseStatus::Mismatch,
        };
        WalletEvent::PayFailed {
            payment_id,
            agent_id,
            token,
            receiver_id,
            amount,
            release,
        }
        .emit();
        false
    }

    // ----- owner methods (exactly 1 yoctoNEAR, predecessor == owner) ------

    /// Create or reconfigure an agent and mark it active. An existing
    /// agent keeps its spend window, so reconfiguring inside a day cannot
    /// reset the daily total.
    #[payable]
    pub fn set_agent(
        &mut self,
        agent_id: AccountId,
        per_call_max: U128,
        daily_max: U128,
        payees: Vec<AccountId>,
    ) {
        self.assert_owner();
        let me = env::current_account_id();
        if agent_id == me || payees.contains(&me) {
            fail_with(ERR_SELF_ACCOUNT, "the wallet cannot be an agent or payee");
        }
        let mut payees = payees;
        payees.sort();
        payees.dedup();
        let names: Vec<String> = payees.iter().map(|p| p.to_string()).collect();
        let policy = match self.agents.get(&agent_id) {
            Some(existing) => {
                let mut policy = existing.policy();
                policy
                    .reconfigure(per_call_max.0, daily_max.0, names)
                    .map(|()| policy)
            }
            None => AgentPolicy::new(per_call_max.0, daily_max.0, names),
        }
        .unwrap_or_else(|e| fail(e));
        let stored = StoredAgent {
            per_call_max: policy.per_call_max(),
            daily_max: policy.daily_max(),
            payees: payees.clone(),
            active: policy.active(),
            spent_in_window: policy.spent_in_window(),
            window_index: policy.window_index(),
        };
        self.agents.insert(agent_id.clone(), stored);
        WalletEvent::AgentSet {
            agent_id,
            per_call_max,
            daily_max,
            payees,
        }
        .emit();
    }

    /// Revoke an agent. Its entry and spend window are kept.
    #[payable]
    pub fn revoke_agent(&mut self, agent_id: AccountId) {
        self.assert_owner();
        let stored = self
            .agents
            .get_mut(&agent_id)
            .unwrap_or_else(|| fail(PolicyError::AgentInactive));
        let mut policy = stored.policy();
        policy.deactivate();
        stored.store_spend(&policy);
        WalletEvent::AgentRevoked { agent_id }.emit();
    }

    /// Replace the token allowlist.
    #[payable]
    pub fn set_tokens(&mut self, tokens: Vec<AccountId>) {
        self.assert_owner();
        self.tokens = normalize_tokens(tokens);
        WalletEvent::TokensSet {
            tokens: self.tokens.clone(),
        }
        .emit();
    }

    /// Pause every agent payment.
    #[payable]
    pub fn pause(&mut self) {
        self.assert_owner();
        self.paused = true;
        WalletEvent::Paused {
            by: self.owner.clone(),
        }
        .emit();
    }

    /// Resume agent payments.
    #[payable]
    pub fn unpause(&mut self) {
        self.assert_owner();
        self.paused = false;
        WalletEvent::Unpaused {
            by: self.owner.clone(),
        }
        .emit();
    }

    /// Transfer `amount` of any token the wallet holds to `to`. Works while
    /// paused and for tokens no longer on the allowlist. Emits `withdrawn`
    /// or `withdraw_failed` from the callback.
    #[payable]
    pub fn withdraw(&mut self, token: AccountId, amount: U128, to: AccountId) -> Promise {
        self.assert_owner();
        if amount.0 == 0 {
            fail(PolicyError::ZeroAmount);
        }
        if to == env::current_account_id() {
            fail_with(ERR_SELF_ACCOUNT, "cannot withdraw to the wallet itself");
        }
        if env::prepaid_gas() < GAS_MIN_PREPAID {
            fail_with(ERR_INSUFFICIENT_GAS, "attach at least 35 TGas");
        }
        ext_ft::ext(token.clone())
            .with_attached_deposit(ONE_YOCTO)
            .with_static_gas(GAS_FT_TRANSFER)
            .ft_transfer(to.clone(), amount, None)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_CALLBACK)
                    .on_withdraw_complete(token, amount, to),
            )
    }

    /// Completion callback for `withdraw`. Returns true on success.
    #[private]
    pub fn on_withdraw_complete(&mut self, token: AccountId, amount: U128, to: AccountId) -> bool {
        if transfer_succeeded() {
            WalletEvent::Withdrawn { token, amount, to }.emit();
            true
        } else {
            WalletEvent::WithdrawFailed { token, amount, to }.emit();
            false
        }
    }

    // ----- funding ----------------------------------------------------------

    /// NEP-141 receiver hook. Accepts the full amount from allowlisted
    /// token contracts and refuses (panics, so the token refunds) any
    /// other.
    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let _ = msg;
        let token = env::predecessor_account_id();
        if !self.tokens.contains(&token) {
            fail(PolicyError::TokenNotAllowed);
        }
        WalletEvent::Funded {
            token,
            sender_id,
            amount,
        }
        .emit();
        PromiseOrValue::Value(U128(0))
    }

    // ----- views ------------------------------------------------------------

    /// An agent's policy as of the current block, or null when unknown.
    pub fn get_agent(&self, agent_id: AccountId) -> Option<AgentView> {
        let now = env::block_timestamp();
        self.agents.get(&agent_id).map(|stored| {
            let policy = stored.policy();
            AgentView {
                per_call_max: U128(stored.per_call_max),
                daily_max: U128(stored.daily_max),
                payees: stored.payees.clone(),
                active: stored.active,
                spent_in_window: U128(policy.spent_at(now)),
                remaining: U128(policy.remaining_at(now)),
                window_index: U64(window_index(now).max(stored.window_index)),
                resets_at_ns: U64(next_window_start(now)),
            }
        })
    }

    /// Spent in the current UTC day by `agent_id`; zero when unknown.
    pub fn get_spent(&self, agent_id: AccountId) -> U128 {
        let now = env::block_timestamp();
        U128(
            self.agents
                .get(&agent_id)
                .map(|stored| stored.policy().spent_at(now))
                .unwrap_or(0),
        )
    }

    /// Wallet configuration.
    pub fn get_config(&self) -> ConfigView {
        ConfigView {
            owner: self.owner.clone(),
            paused: self.paused,
            tokens: self.tokens.clone(),
            next_payment_id: U64(self.next_payment_id),
            event_standard: events::EVENT_STANDARD.to_string(),
            event_version: events::EVENT_VERSION.to_string(),
        }
    }
}

impl PolicyWallet {
    /// Exactly 1 yoctoNEAR attached and the owner as predecessor. The
    /// deposit check runs first: a function-call key can never satisfy it.
    fn assert_owner(&self) {
        assert_one_yocto();
        if env::predecessor_account_id() != self.owner {
            fail_with(ERR_NOT_OWNER, "only the owner can call this method");
        }
    }
}

/// True unless the single promise this callback depends on failed. A
/// non-empty success payload counts as success; it is not read.
fn transfer_succeeded() -> bool {
    !matches!(env::promise_result_checked(0, 0), Err(PromiseError::Failed))
}
