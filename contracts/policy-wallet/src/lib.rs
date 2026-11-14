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
//! upgrade path. The owner can `close` a paused wallet whose allowlisted
//! token balances are all zero; that deletes the wallet account and returns
//! its NEAR, including the storage stake, to a beneficiary.

#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::too_many_lines
)]

pub mod events;

#[cfg(test)]
mod properties;
#[cfg(test)]
mod tests;

use events::{ReleaseStatus, TokenBalance, WalletEvent};
use near_sdk::json_types::{U128, U64};
use near_sdk::store::LookupMap;
use near_sdk::{
    assert_one_yocto, env, ext_contract, near, AccountId, BorshStorageKey, Gas, NearToken,
    PanicOnDefault, Promise, PromiseError, PromiseOrValue,
};
use policy_core::{
    next_window_start, window_index, AgentPolicy, HoldQuota, PolicyError, ReleaseOutcome,
    Reservation, WalletGate, MAX_DAILY_HOLDS_PER_AGENT, MAX_OPEN_HOLDS_PER_AGENT, MAX_PAYEES,
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
/// `new` was called by someone other than this account or its parent.
pub const ERR_INIT_NOT_ALLOWED: &str = "ERR_INIT_NOT_ALLOWED";
/// `close` was called while the wallet is not paused.
pub const ERR_NOT_PAUSED: &str = "ERR_NOT_PAUSED";
/// The owner-approval queue already holds [`MAX_PENDING`] payments.
pub const ERR_REVIEW_QUEUE_FULL: &str = "ERR_REVIEW_QUEUE_FULL";
/// `approve_payment` or `reject_payment` named an id that is not waiting.
pub const ERR_NOT_PENDING: &str = "ERR_NOT_PENDING";

/// Largest number of payments waiting for the owner.
pub const MAX_PENDING: usize = 8;
/// Queued because the payee is not on the allowlist.
pub const REASON_NEW_PAYEE: &str = "new_payee";
/// Queued because the amount is above the owner's approval threshold.
pub const REASON_ABOVE_THRESHOLD: &str = "above_threshold";

/// Largest accepted memo, in bytes.
pub const MAX_MEMO_LEN: usize = 256;
/// Largest token allowlist.
///
/// Limits and the running total are kept per token. A cap sized for
/// wNEAR (24 decimals) is not a cap on USDC (6 decimals): each token has
/// its own `per_call_max`, `daily_max` and window. With one token,
/// `set_agent` writes that token's limits. With two or more, `set_agent`
/// only edits the payee list and `set_token_limits` sets each token.
pub const MAX_TOKENS: usize = 8;

/// Circle native USDC on NEAR mainnet (NEP-141, 6 decimals).
///
/// This is the contract Circle publishes as native USDC, not bridged
/// USDC.e (`a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.factory.bridge.near`).
/// Source: https://www.circle.com/blog/usdc-is-now-available-on-near
/// (token address `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`)
/// and https://developers.circle.com/stablecoins/usdc-contract-addresses .
pub const USDC_MAINNET: &str = "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";
/// `wrap.near`, the NEP-141 wrapper for native NEAR (24 decimals).
pub const WRAP_NEAR: &str = "wrap.near";
/// Beta ceiling for native USDC: 100 USDC, in 6-decimal atomic units.
pub const BETA_USDC_CAP: u128 = 100_000_000;
/// Beta ceiling for wNEAR: 10 wNEAR, in 24-decimal atomic units.
pub const BETA_WNEAR_CAP: u128 = 10_000_000_000_000_000_000_000_000;
/// A queued payment is dropped after this long. It never reserved spend.
pub const HOLD_TTL_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

/// `set_agent` was asked for one pair of limits while several tokens are
/// allowlisted. Those numbers are not a unit, so they are refused.
pub const ERR_AMBIGUOUS_LIMITS: &str = "ERR_AMBIGUOUS_LIMITS";
/// The deposit would put the wallet over its beta cap for that token.
pub const ERR_BETA_CAP: &str = "ERR_BETA_CAP";
/// The token is allowlisted, but this agent has no limit for it yet.
pub const ERR_TOKEN_LIMITS_UNSET: &str = "ERR_TOKEN_LIMITS_UNSET";

/// Gas attached to each `ft_transfer`.
pub const GAS_FT_TRANSFER: Gas = Gas::from_tgas(15);
/// Gas attached to each completion callback.
pub const GAS_CALLBACK: Gas = Gas::from_tgas(10);
/// Minimum prepaid gas for `pay` and `withdraw`: the transfer, the callback
/// and headroom for the calling method itself. Below this the callback
/// could run out of gas and leave a reservation unreleased.
pub const GAS_MIN_PREPAID: Gas = Gas::from_tgas(35);
/// Gas attached to each `ft_balance_of` query made by `close`.
pub const GAS_BALANCE_QUERY: Gas = Gas::from_tgas(5);
/// Gas attached to the `close` callback (balance checks plus the delete).
pub const GAS_CLOSE_CALLBACK: Gas = Gas::from_tgas(15);
/// Headroom `close` keeps for its own execution on top of the queries and
/// the callback.
pub const GAS_CLOSE_OVERHEAD: Gas = Gas::from_tgas(10);
/// Largest `ft_balance_of` result accepted (a JSON-quoted u128).
const MAX_BALANCE_RESULT_LEN: usize = 48;

/// One yoctoNEAR, the deposit NEP-141 `ft_transfer` requires.
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

/// The NEP-141 methods the wallet calls on token contracts.
#[ext_contract(ext_ft)]
pub trait FungibleToken {
    /// NEP-141 transfer; requires exactly 1 yoctoNEAR attached.
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
    /// NEP-141 balance query.
    fn ft_balance_of(&self, account_id: AccountId) -> U128;
}

#[near]
#[derive(BorshStorageKey)]
enum StorageKey {
    Agents,
    Pending,
}

/// Limits and spend for one token. Atomic units are that token's own.
#[near(serializers = [borsh, json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenBudget {
    /// The NEP-141 contract these numbers are denominated in.
    pub token: AccountId,
    per_call_max: u128,
    daily_max: u128,
    spent_in_window: u128,
    window_index: u64,
}

/// An agent's stored policy. Limits and spend live in `budgets`, one entry
/// per token, keyed by the token id, on single-token and multi-token
/// wallets alike. Changing the allowlist therefore never moves, resets or
/// re-labels a token's spend window (ToB C-SC-4, C-VA-2). Hold counters
/// never move spend.
#[near(serializers = [borsh])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgent {
    payees: Vec<AccountId>,
    active: bool,
    /// Open holds. Mirrors [`HoldQuota::open`].
    holds_open: u32,
    /// Holds opened in `holds_window`. Rejecting one does not decrease it.
    holds_created: u32,
    /// UTC day `holds_created` belongs to.
    holds_window: u64,
    /// Per-token limits and spend. A token without an entry cannot be spent.
    budgets: Vec<TokenBudget>,
}

/// Beta custody of one token: the cap, and how much `ft_on_transfer` has
/// accepted minus successful withdrawals. A direct `ft_transfer` does not
/// pass through this counter.
#[near(serializers = [borsh])]
#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenCap {
    token: AccountId,
    max_balance: u128,
    custody: u128,
}

impl StoredAgent {
    fn budget(&self, token: &AccountId) -> Option<&TokenBudget> {
        self.budgets.iter().find(|b| &b.token == token)
    }

    fn hold_quota(&self) -> HoldQuota {
        HoldQuota::from_parts(self.holds_open, self.holds_created, self.holds_window)
    }

    fn store_quota(&mut self, quota: HoldQuota) {
        self.holds_open = quota.open();
        self.holds_created = quota.created_in_window();
        self.holds_window = quota.window_index();
    }

    fn allows_payee(&self, payee: &str) -> bool {
        self.payees.iter().any(|p| p.as_str() == payee)
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
    /// Per-token limits. One entry when the wallet has a single token.
    pub budgets: Vec<TokenBudgetView>,
    /// Holds this agent currently has waiting.
    pub open_holds: u32,
    /// Holds this agent has opened in the current UTC day.
    pub holds_today: u32,
}

/// One token's limits and spend, as of the current block.
#[near(serializers = [json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenBudgetView {
    /// The NEP-141 token contract.
    pub token: AccountId,
    /// Largest single payment, in this token's atomic units.
    pub per_call_max: U128,
    /// Largest total per UTC day, in this token's atomic units.
    pub daily_max: U128,
    /// Spent in the current UTC day.
    pub spent_in_window: U128,
    /// Remaining budget in the current UTC day.
    pub remaining: U128,
    /// Current UTC day number.
    pub window_index: U64,
}

/// A payment reserved against the daily limit and not yet transferred.
#[near(serializers = [borsh, json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPayment {
    /// The agent that called `pay`.
    pub agent_id: AccountId,
    /// The NEP-141 token contract.
    pub token: AccountId,
    /// The payee.
    pub receiver_id: AccountId,
    /// Amount in token atomic units.
    pub amount: u128,
    /// Memo to use if the owner approves.
    pub memo: Option<String>,
    /// `new_payee` or `above_threshold`.
    pub reason: String,
    /// UTC day the hold was checked against. Spend is reserved only on approve.
    pub window_index: u64,
    /// Block timestamp (ns) when the hold was queued.
    pub created_at_ns: u64,
}

/// One queued payment, as returned by `get_pending`.
#[near(serializers = [json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingView {
    /// Id to pass to `approve_payment` or `reject_payment`.
    pub payment_id: U64,
    /// The agent that called `pay`.
    pub agent_id: AccountId,
    /// The NEP-141 token contract.
    pub token: AccountId,
    /// The payee.
    pub receiver_id: AccountId,
    /// Amount in token atomic units.
    pub amount: U128,
    /// Memo that an approval will pass to `ft_transfer`.
    pub memo: Option<String>,
    /// `new_payee` or `above_threshold`.
    pub reason: String,
    /// UTC day the hold was checked against. Spend is reserved only on approve.
    pub window_index: U64,
    /// Block timestamp (ns) when the hold was queued.
    pub created_at_ns: U64,
    /// Block timestamp (ns) when the hold expires if still waiting.
    pub expires_at_ns: U64,
}

/// A beta deposit cap and how much of it `ft_on_transfer` has used.
#[near(serializers = [json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositCapView {
    /// The NEP-141 token contract.
    pub token: AccountId,
    /// Maximum custody, in atomic units. The owner cannot raise a built-in cap.
    pub max_balance: U128,
    /// Amount accepted through `ft_on_transfer` and not yet withdrawn.
    pub custody: U128,
}

/// Owner-approval settings.
#[near(serializers = [json])]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewView {
    /// Amounts strictly above this wait for the owner. Zero disables it.
    pub approval_threshold: U128,
    /// When true, a payee outside the allowlist is queued instead of refused.
    pub review_unknown_payees: bool,
    /// Payments currently waiting.
    pub pending_count: u64,
    /// Queue capacity.
    pub max_pending: u64,
    /// Open holds one agent may have at once.
    pub max_open_holds_per_agent: u32,
    /// Holds one agent may open per UTC day.
    pub max_daily_holds_per_agent: u32,
    /// Nanoseconds after which a hold is dropped.
    pub hold_ttl_ns: U64,
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
    /// Beta deposit caps. Empty when no allowlisted token has a cap.
    pub deposit_caps: Vec<DepositCapView>,
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
    /// Payments waiting for `approve_payment` or `reject_payment`.
    pending: LookupMap<u64, PendingPayment>,
    /// Queue order, oldest first. Bounded by [`MAX_PENDING`].
    pending_order: Vec<u64>,
    /// Zero means an allowlisted payment is never queued for its amount.
    approval_threshold: u128,
    /// When true, `pay` to a payee outside the allowlist queues the payment.
    review_unknown_payees: bool,
    /// Beta caps and custody. Tokens without an entry are uncapped.
    caps: Vec<TokenCap>,
}

fn fail(err: PolicyError) -> ! {
    env::panic_str(&err.to_string())
}

fn fail_with(code: &str, description: &str) -> ! {
    env::panic_str(&format!("{code}: {description}"))
}

/// `new` is allowed from this account, or from its parent. The parent is the
/// factory when the wallet was created as `<name>.<factory>`.
fn assert_initializer() {
    let me = env::current_account_id();
    let predecessor = env::predecessor_account_id();
    if predecessor == me {
        return;
    }
    let parent_ok = me
        .as_str()
        .split_once('.')
        .is_some_and(|(_, parent)| predecessor.as_str() == parent);
    if !parent_ok {
        fail_with(
            ERR_INIT_NOT_ALLOWED,
            "new may only be called by this account or its parent",
        );
    }
}

fn normalize_tokens(mut tokens: Vec<AccountId>) -> Vec<AccountId> {
    tokens.sort();
    tokens.dedup();
    if tokens.len() > MAX_TOKENS {
        fail_with(
            ERR_TOO_MANY_TOKENS,
            "at most 8 tokens; each token has its own limits",
        );
    }
    if tokens.contains(&env::current_account_id()) {
        fail_with(ERR_SELF_ACCOUNT, "the wallet cannot be its own token");
    }
    tokens
}

#[near]
impl PolicyWallet {
    /// Initialize the wallet. Callable once.
    ///
    /// A direct deploy puts `new` in the same transaction as the deploy, signed
    /// by this account. The factory puts `new` in the same receipt as
    /// `create_account` and `deploy_contract`. In that receipt the predecessor
    /// is the factory, which is this account's parent. Anyone else is refused,
    /// so a two-step deploy cannot be front-run. `#[init]` still rejects a
    /// second call.
    /// `tokens` holds at most [`MAX_TOKENS`] contracts. Native USDC and
    /// `wrap.near` start with the beta deposit cap; any other token is
    /// uncapped until `set_deposit_cap`.
    #[init]
    pub fn new(owner: AccountId, tokens: Vec<AccountId>) -> Self {
        assert_initializer();
        if owner == env::current_account_id() {
            fail_with(ERR_SELF_ACCOUNT, "the wallet cannot own itself");
        }
        let tokens = normalize_tokens(tokens);
        let caps = initial_caps(&tokens);
        Self {
            owner,
            paused: false,
            tokens,
            agents: LookupMap::new(StorageKey::Agents),
            next_payment_id: 0,
            pending: LookupMap::new(StorageKey::Pending),
            pending_order: Vec::new(),
            approval_threshold: 0,
            review_unknown_payees: false,
            caps,
        }
    }

    // ----- agent method ---------------------------------------------------

    /// Pay `amount` of `token` to `receiver_id` on behalf of the calling
    /// agent. Zero deposit; at least [`GAS_MIN_PREPAID`] gas.
    ///
    /// Refusals panic with a stable code and change no state: `ERR_PAUSED`,
    /// `ERR_AGENT_INACTIVE`, `ERR_TOKEN_NOT_ALLOWED`, `ERR_PAYEE_NOT_ALLOWED`,
    /// `ERR_ZERO_AMOUNT`, `ERR_PER_CALL_LIMIT`, `ERR_DAILY_LIMIT`,
    /// `ERR_OVERFLOW`, `ERR_REVIEW_QUEUE_FULL`, `ERR_AGENT_OPEN_HOLDS`,
    /// `ERR_AGENT_DAILY_HOLDS`, `ERR_TOKEN_LIMITS_UNSET`, plus
    /// `ERR_DEPOSIT_NOT_ZERO`, `ERR_MEMO_TOO_LONG` and `ERR_INSUFFICIENT_GAS`.
    ///
    /// When the owner has turned review on, a new payee or an amount above
    /// the threshold is queued instead of transferred. The hold is checked
    /// against the caps but does not consume the daily cap. The return is
    /// then the payment id. Otherwise the return is the transfer promise.
    /// Review is off until `set_review_policy`. Expired holds are dropped
    /// before the queue cap is applied.
    pub fn pay(
        &mut self,
        token: AccountId,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
    ) -> PromiseOrValue<U64> {
        if !env::attached_deposit().is_zero() {
            fail_with(ERR_DEPOSIT_NOT_ZERO, "pay takes no deposit");
        }
        if memo.as_ref().is_some_and(|m| m.len() > MAX_MEMO_LEN) {
            fail_with(ERR_MEMO_TOO_LONG, "memo exceeds 256 bytes");
        }
        if self.paused {
            fail(PolicyError::Paused);
        }
        self.sweep_expired();
        let agent_id = env::predecessor_account_id();
        let gate = WalletGate {
            paused: self.paused,
            token_allowed: self.tokens.contains(&token),
        };
        let now = env::block_timestamp();
        let allowlisted = self
            .agents
            .get(&agent_id)
            .unwrap_or_else(|| fail(PolicyError::AgentInactive))
            .allows_payee(receiver_id.as_str());
        if !gate.token_allowed {
            fail(PolicyError::TokenNotAllowed);
        }
        let hold_new = self.review_unknown_payees && !allowlisted;
        let hold_amount =
            allowlisted && self.approval_threshold > 0 && amount.0 > self.approval_threshold;
        if hold_new || hold_amount {
            if receiver_id == env::current_account_id() {
                fail_with(ERR_SELF_ACCOUNT, "the wallet cannot be a payee");
            }
            if self.pending_order.len() >= MAX_PENDING {
                fail_with(
                    ERR_REVIEW_QUEUE_FULL,
                    "the owner still has payments waiting",
                );
            }
            let open_now = self.open_holds(&agent_id);
            let (policy, mut quota) = {
                let stored = self
                    .agents
                    .get(&agent_id)
                    .unwrap_or_else(|| fail(PolicyError::AgentInactive));
                let policy = load_policy(stored, &token).unwrap_or_else(|| {
                    fail_with(
                        ERR_TOKEN_LIMITS_UNSET,
                        "set a per-token limit before paying this token",
                    )
                });
                (policy, stored.hold_quota())
            };
            // Checked, not reserved. A later approve consumes the cap.
            if hold_new {
                policy
                    .check_unlisted(gate, amount.0, now)
                    .unwrap_or_else(|e| fail(e));
            } else {
                policy
                    .check(gate, receiver_id.as_str(), amount.0, now)
                    .unwrap_or_else(|e| fail(e));
            }
            if open_now >= MAX_OPEN_HOLDS_PER_AGENT as usize {
                fail(PolicyError::AgentOpenHolds);
            }
            quota.try_open(now).unwrap_or_else(|e| fail(e));
            let stored = self
                .agents
                .get_mut(&agent_id)
                .unwrap_or_else(|| fail(PolicyError::AgentInactive));
            stored.store_quota(quota);
            let payment_id = self.alloc_payment_id();
            let reason = if hold_new {
                REASON_NEW_PAYEE
            } else {
                REASON_ABOVE_THRESHOLD
            };
            self.pending.insert(
                payment_id,
                PendingPayment {
                    agent_id: agent_id.clone(),
                    token: token.clone(),
                    receiver_id: receiver_id.clone(),
                    amount: amount.0,
                    memo: memo.clone(),
                    reason: reason.to_string(),
                    window_index: window_index(now),
                    created_at_ns: now,
                },
            );
            self.pending_order.push(payment_id);
            WalletEvent::PaymentQueued {
                payment_id: U64(payment_id),
                agent_id,
                token,
                receiver_id,
                amount,
                reason: reason.to_string(),
            }
            .emit();
            return PromiseOrValue::Value(U64(payment_id));
        }

        if env::prepaid_gas() < GAS_MIN_PREPAID {
            fail_with(ERR_INSUFFICIENT_GAS, "attach at least 35 TGas");
        }
        let stored = self
            .agents
            .get_mut(&agent_id)
            .unwrap_or_else(|| fail(PolicyError::AgentInactive));
        let mut policy = load_policy(stored, &token).unwrap_or_else(|| {
            fail_with(
                ERR_TOKEN_LIMITS_UNSET,
                "set a per-token limit before paying this token",
            )
        });
        let reservation = policy
            .check_and_reserve(gate, receiver_id.as_str(), amount.0, now)
            .unwrap_or_else(|e| fail(e));
        write_spend(stored, &token, &policy);
        let payment_id = self.alloc_payment_id();
        PromiseOrValue::Promise(Self::schedule_transfer(ScheduledTransfer {
            agent_id,
            token,
            receiver_id,
            amount,
            memo,
            payment_id,
            window_index: reservation.window_index,
        }))
    }

    /// Completion callback for `pay`. Emits `pay_settled` on success; on
    /// failure releases the reservation and emits `pay_failed`. Never
    /// panics, so its state change is never reverted. Returns true when the
    /// transfer settled. A settled payment lowers the token's beta custody
    /// counter, so paying out frees deposit headroom (ToB C-SC-6).
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
            if let Some(cap) = self.caps.iter_mut().find(|c| c.token == token) {
                cap.custody = cap.custody.saturating_sub(amount.0);
            }
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
            Some(stored) => match load_policy(stored, &token) {
                Some(mut policy) => {
                    let reservation = Reservation {
                        amount: amount.0,
                        window_index: window_index.0,
                    };
                    match policy.release(reservation) {
                        Ok(ReleaseOutcome::Restored) => {
                            write_spend(stored, &token, &policy);
                            ReleaseStatus::Restored
                        }
                        Ok(ReleaseOutcome::WindowExpired) => ReleaseStatus::WindowExpired,
                        Err(_) => ReleaseStatus::Mismatch,
                    }
                }
                None => ReleaseStatus::Mismatch,
            },
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
    /// reset the daily total. With exactly one allowlisted token the limits
    /// apply to that token; otherwise pass zero limits and call
    /// `set_token_limits` once per token. At most `MAX_PAYEES` payees.
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
        // Both branches refuse an oversized payee list (ToB C-ACB-2).
        if payees.len() > MAX_PAYEES {
            fail(PolicyError::InvalidPolicy);
        }
        let now = env::block_timestamp();
        let tokens = self.tokens.clone();
        let mut stored = self
            .agents
            .get(&agent_id)
            .cloned()
            .unwrap_or_else(empty_agent);
        stored.payees.clone_from(&payees);
        if let [token] = tokens.as_slice() {
            set_limits(
                &mut stored,
                &tokens,
                token,
                per_call_max.0,
                daily_max.0,
                now,
            )
            .unwrap_or_else(|e| fail(e));
        } else if per_call_max.0 != 0 || daily_max.0 != 0 {
            fail_with(ERR_AMBIGUOUS_LIMITS, "call set_token_limits once per token");
        }
        stored.active = true;
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
        stored.active = false;
        WalletEvent::AgentRevoked { agent_id }.emit();
    }

    /// Set one token's per-call and daily limits for an agent that already
    /// exists. That token's spend in the current window is kept. The token
    /// must be on the wallet allowlist. This does not change whether the
    /// agent is active: a revoked agent stays revoked until `set_agent`
    /// (ToB C-SE-1).
    #[payable]
    pub fn set_token_limits(
        &mut self,
        agent_id: AccountId,
        token: AccountId,
        per_call_max: U128,
        daily_max: U128,
    ) {
        self.assert_owner();
        if !self.tokens.contains(&token) {
            fail(PolicyError::TokenNotAllowed);
        }
        let now = env::block_timestamp();
        let tokens = self.tokens.clone();
        let stored = self
            .agents
            .get_mut(&agent_id)
            .unwrap_or_else(|| fail(PolicyError::AgentInactive));
        set_limits(stored, &tokens, &token, per_call_max.0, daily_max.0, now)
            .unwrap_or_else(|e| fail(e));
    }

    /// Replace the token allowlist (at most [`MAX_TOKENS`]).
    ///
    /// Each token keeps its own limits. A token with no `set_token_limits`
    /// (and, when it is the only token, no `set_agent` limits) cannot be
    /// spent. Custody of a removed token is kept so a withdrawal still
    /// lowers the beta counter.
    #[payable]
    pub fn set_tokens(&mut self, tokens: Vec<AccountId>) {
        self.assert_owner();
        let tokens = normalize_tokens(tokens);
        let mut caps = Vec::new();
        for token in &tokens {
            if let Some(existing) = self.caps.iter().find(|c| &c.token == token) {
                caps.push(existing.clone());
            } else if let Some(max_balance) = builtin_cap(token) {
                caps.push(TokenCap {
                    token: token.clone(),
                    max_balance,
                    custody: 0,
                });
            }
        }
        for old in &self.caps {
            if !tokens.contains(&old.token) && old.custody > 0 {
                caps.push(old.clone());
            }
        }
        self.tokens = tokens;
        self.caps = caps;
        WalletEvent::TokensSet {
            tokens: self.tokens.clone(),
        }
        .emit();
    }

    /// Lower, or set, the beta deposit cap for an allowlisted token.
    ///
    /// Native USDC and `wrap.near` have a built-in ceiling. The owner can
    /// set a lower cap and can raise it back up to that ceiling, not past
    /// it. Any other token starts uncapped; the owner may set a cap of
    /// their own.
    #[payable]
    pub fn set_deposit_cap(&mut self, token: AccountId, max_balance: U128) {
        self.assert_owner();
        if !self.tokens.contains(&token) {
            fail(PolicyError::TokenNotAllowed);
        }
        if let Some(hard) = builtin_cap(&token) {
            if max_balance.0 > hard {
                fail_with(ERR_BETA_CAP, "cannot raise the beta cap");
            }
        }
        if let Some(cap) = self.caps.iter_mut().find(|c| c.token == token) {
            cap.max_balance = max_balance.0;
        } else {
            self.caps.push(TokenCap {
                token,
                max_balance: max_balance.0,
                custody: 0,
            });
        }
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

    /// Set when `pay` queues a payment instead of transferring it.
    ///
    /// `approval_threshold` of zero disables the amount rule. A positive
    /// threshold queues an allowlisted payment whose amount is strictly
    /// greater. `review_unknown_payees` queues a payee that is not on the
    /// allowlist; while it is false, that payee is still `ERR_PAYEE_NOT_ALLOWED`.
    /// Either way the per-call limit, the daily limit, the token allowlist,
    /// pause, and revocation still apply. A hold is checked against those
    /// caps and does not consume the daily cap; `approve_payment` checks
    /// them again and reserves the amount then.
    #[payable]
    pub fn set_review_policy(&mut self, approval_threshold: U128, review_unknown_payees: bool) {
        self.assert_owner();
        self.approval_threshold = approval_threshold.0;
        self.review_unknown_payees = review_unknown_payees;
    }

    /// Pay a queued payment. Owner only. The caps are checked again and the
    /// amount is reserved now; a failed transfer releases it in
    /// `on_pay_complete`. Refuses while the wallet is paused, the agent is
    /// inactive, or the caps no longer allow the amount, and leaves the
    /// queue entry in place so the owner can `reject_payment` instead.
    #[payable]
    pub fn approve_payment(&mut self, payment_id: U64) -> Promise {
        self.assert_owner();
        if env::prepaid_gas() < GAS_MIN_PREPAID {
            fail_with(ERR_INSUFFICIENT_GAS, "attach at least 35 TGas");
        }
        if self.paused {
            fail(PolicyError::Paused);
        }
        self.sweep_expired();
        let pending =
            self.pending.get(&payment_id.0).cloned().unwrap_or_else(|| {
                fail_with(ERR_NOT_PENDING, "no payment is waiting with that id")
            });
        let now = env::block_timestamp();
        let gate = WalletGate {
            paused: self.paused,
            token_allowed: self.tokens.contains(&pending.token),
        };
        let reservation = {
            let stored = self
                .agents
                .get(&pending.agent_id)
                .unwrap_or_else(|| fail(PolicyError::AgentInactive));
            if !stored.active {
                fail(PolicyError::AgentInactive);
            }
            let mut policy = load_policy(stored, &pending.token).unwrap_or_else(|| {
                fail_with(
                    ERR_TOKEN_LIMITS_UNSET,
                    "set a per-token limit before approving this token",
                )
            });
            // The owner is authorizing the payee. Caps still apply.
            let reservation = policy
                .check_and_reserve_unlisted(gate, pending.amount, now)
                .unwrap_or_else(|e| fail(e));
            (policy, reservation)
        };
        let (policy, reservation) = reservation;
        let stored = self
            .agents
            .get_mut(&pending.agent_id)
            .unwrap_or_else(|| fail(PolicyError::AgentInactive));
        write_spend(stored, &pending.token, &policy);
        let mut quota = stored.hold_quota();
        quota.close_one();
        stored.store_quota(quota);
        self.take_pending(payment_id.0);
        Self::schedule_transfer(ScheduledTransfer {
            agent_id: pending.agent_id,
            token: pending.token,
            receiver_id: pending.receiver_id,
            amount: U128(pending.amount),
            memo: pending.memo,
            payment_id: payment_id.0,
            window_index: reservation.window_index,
        })
    }

    /// Drop a queued payment. A hold never reserved daily budget, so the
    /// agent's spend does not change. Works while paused.
    #[payable]
    pub fn reject_payment(&mut self, payment_id: U64) {
        self.assert_owner();
        self.sweep_expired();
        self.drop_hold(payment_id.0);
    }

    /// Drop each listed hold. Unknown ids fail the call. At most
    /// [`MAX_PENDING`] ids. Duplicates are ignored after the first.
    #[payable]
    pub fn reject_payments(&mut self, payment_ids: Vec<U64>) {
        self.assert_owner();
        if payment_ids.len() > MAX_PENDING {
            fail_with(ERR_REVIEW_QUEUE_FULL, "too many payment ids");
        }
        self.sweep_expired();
        let mut seen = Vec::new();
        for id in payment_ids {
            if seen.contains(&id.0) {
                continue;
            }
            seen.push(id.0);
            self.drop_hold(id.0);
        }
    }

    /// Drop every hold. Returns how many were dropped.
    #[payable]
    pub fn reject_all(&mut self) -> U64 {
        self.assert_owner();
        self.sweep_expired();
        let ids = self.pending_order.clone();
        let n = ids.len() as u64;
        for id in ids {
            self.drop_hold(id);
        }
        U64(n)
    }

    /// Drop holds older than [`HOLD_TTL_NS`]. Anyone may call this: it moves
    /// no tokens and frees queue slots. Returns how many were dropped.
    pub fn expire_holds(&mut self) -> U64 {
        let before = self.pending_order.len();
        self.sweep_expired();
        U64((before - self.pending_order.len()) as u64)
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
            if let Some(cap) = self.caps.iter_mut().find(|c| c.token == token) {
                cap.custody = cap.custody.saturating_sub(amount.0);
            }
            WalletEvent::Withdrawn { token, amount, to }.emit();
            true
        } else {
            WalletEvent::WithdrawFailed { token, amount, to }.emit();
            false
        }
    }

    // ----- close ------------------------------------------------------------

    /// Delete the wallet account and send its NEAR (including the storage
    /// stake) to `beneficiary`. Owner only, exactly 1 yoctoNEAR, and only
    /// while paused. Queries `ft_balance_of(wallet)` on every allowlisted
    /// token; the callback deletes the account only if every balance is
    /// zero, and otherwise emits `close_refused` and changes nothing.
    ///
    /// Tokens held but no longer allowlisted are not checked: withdraw them
    /// first. `beneficiary` must be an existing account; NEAR sent by an
    /// account deletion to a missing account is not recoverable.
    #[payable]
    pub fn close(&mut self, beneficiary: AccountId) -> PromiseOrValue<bool> {
        self.assert_owner();
        if !self.paused {
            fail_with(ERR_NOT_PAUSED, "pause the wallet before closing");
        }
        let me = env::current_account_id();
        if beneficiary == me {
            fail_with(ERR_SELF_ACCOUNT, "the wallet cannot be its own beneficiary");
        }
        let tokens = self.tokens.clone();
        let Some((first, rest)) = tokens.split_first() else {
            // Nothing to check: close at once.
            WalletEvent::Closed {
                beneficiary: beneficiary.clone(),
            }
            .emit();
            return PromiseOrValue::Promise(Promise::new(me).delete_account(beneficiary));
        };
        let needed = GAS_BALANCE_QUERY
            .saturating_mul(tokens.len() as u64)
            .saturating_add(GAS_CLOSE_CALLBACK)
            .saturating_add(GAS_CLOSE_OVERHEAD);
        if env::prepaid_gas() < needed {
            fail_with(
                ERR_INSUFFICIENT_GAS,
                "attach 25 TGas plus 5 TGas per allowlisted token",
            );
        }
        let query = |token: &AccountId| {
            ext_ft::ext(token.clone())
                .with_static_gas(GAS_BALANCE_QUERY)
                .ft_balance_of(me.clone())
        };
        let balances = rest
            .iter()
            .fold(query(first), |all, token| all.and(query(token)));
        PromiseOrValue::Promise(
            balances.then(
                Self::ext(me.clone())
                    .with_static_gas(GAS_CLOSE_CALLBACK)
                    .on_close_check(beneficiary, tokens),
            ),
        )
    }

    /// Callback for `close`. Deletes the wallet account when every queried
    /// balance is zero; a failed or unreadable query counts as non-zero.
    #[private]
    pub fn on_close_check(
        &mut self,
        beneficiary: AccountId,
        tokens: Vec<AccountId>,
    ) -> PromiseOrValue<bool> {
        let held: Vec<TokenBalance> = tokens
            .into_iter()
            .enumerate()
            .filter_map(|(i, token)| {
                let balance = env::promise_result_checked(i as u64, MAX_BALANCE_RESULT_LEN)
                    .ok()
                    .and_then(|bytes| near_sdk::serde_json::from_slice::<U128>(&bytes).ok());
                match balance {
                    Some(U128(0)) => None,
                    other => Some(TokenBalance {
                        token,
                        balance: other,
                    }),
                }
            })
            .collect();
        if !held.is_empty() {
            WalletEvent::CloseRefused { tokens: held }.emit();
            return PromiseOrValue::Value(false);
        }
        WalletEvent::Closed {
            beneficiary: beneficiary.clone(),
        }
        .emit();
        PromiseOrValue::Promise(Promise::new(env::current_account_id()).delete_account(beneficiary))
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
        if let Some(cap) = self.caps.iter_mut().find(|c| c.token == token) {
            let next = cap
                .custody
                .checked_add(amount.0)
                .unwrap_or_else(|| fail_with(ERR_BETA_CAP, "beta deposit cap exceeded"));
            if next > cap.max_balance {
                fail_with(ERR_BETA_CAP, "beta deposit cap exceeded");
            }
            cap.custody = next;
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
            let budgets = self.budget_views(stored, now);
            // The top-level numbers describe the single allowlisted token;
            // they are zero when limits are per token.
            let single = self
                .single_budget(stored)
                .map(|b| budget_view(stored, b, now));
            let holds_today = holds_today(stored, now);
            AgentView {
                per_call_max: single.as_ref().map_or(U128(0), |v| v.per_call_max),
                daily_max: single.as_ref().map_or(U128(0), |v| v.daily_max),
                payees: stored.payees.clone(),
                active: stored.active,
                spent_in_window: single.as_ref().map_or(U128(0), |v| v.spent_in_window),
                remaining: single.as_ref().map_or(U128(0), |v| v.remaining),
                window_index: single
                    .as_ref()
                    .map_or(U64(window_index(now)), |v| v.window_index),
                resets_at_ns: U64(next_window_start(now)),
                budgets,
                open_holds: stored.holds_open,
                holds_today,
            }
        })
    }

    /// Spent in the current UTC day by `agent_id` on the single-token budget;
    /// zero when unknown or when limits are per token. Use [`get_token_spent`]
    /// for one token.
    pub fn get_spent(&self, agent_id: AccountId) -> U128 {
        let now = env::block_timestamp();
        let Some(stored) = self.agents.get(&agent_id) else {
            return U128(0);
        };
        U128(self.single_budget(stored).map_or(0, |budget| {
            budget_view(stored, budget, now).spent_in_window.0
        }))
    }

    /// Spent in the current UTC day by `agent_id` on `token`.
    pub fn get_token_spent(&self, agent_id: AccountId, token: AccountId) -> U128 {
        let now = env::block_timestamp();
        let Some(stored) = self.agents.get(&agent_id) else {
            return U128(0);
        };
        U128(load_policy(stored, &token).map_or(0, |policy| policy.spent_at(now)))
    }

    /// Payments waiting for the owner, oldest first.
    pub fn get_pending(&self) -> Vec<PendingView> {
        self.pending_order
            .iter()
            .filter_map(|id| {
                self.pending.get(id).map(|pending| PendingView {
                    payment_id: U64(*id),
                    agent_id: pending.agent_id.clone(),
                    token: pending.token.clone(),
                    receiver_id: pending.receiver_id.clone(),
                    amount: U128(pending.amount),
                    memo: pending.memo.clone(),
                    reason: pending.reason.clone(),
                    window_index: U64(pending.window_index),
                    created_at_ns: U64(pending.created_at_ns),
                    expires_at_ns: U64(pending.created_at_ns.saturating_add(HOLD_TTL_NS)),
                })
            })
            .collect()
    }

    /// The owner-approval settings. Both switches default to off.
    pub fn get_review_policy(&self) -> ReviewView {
        ReviewView {
            approval_threshold: U128(self.approval_threshold),
            review_unknown_payees: self.review_unknown_payees,
            pending_count: self.pending_order.len() as u64,
            max_pending: MAX_PENDING as u64,
            max_open_holds_per_agent: MAX_OPEN_HOLDS_PER_AGENT,
            max_daily_holds_per_agent: MAX_DAILY_HOLDS_PER_AGENT,
            hold_ttl_ns: U64(HOLD_TTL_NS),
        }
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
            deposit_caps: self
                .caps
                .iter()
                .map(|cap| DepositCapView {
                    token: cap.token.clone(),
                    max_balance: U128(cap.max_balance),
                    custody: U128(cap.custody),
                })
                .collect(),
        }
    }
}

struct ScheduledTransfer {
    agent_id: AccountId,
    token: AccountId,
    receiver_id: AccountId,
    amount: U128,
    memo: Option<String>,
    payment_id: u64,
    window_index: u64,
}

impl PolicyWallet {
    fn alloc_payment_id(&mut self) -> u64 {
        let payment_id = self.next_payment_id;
        self.next_payment_id = payment_id
            .checked_add(1)
            .unwrap_or_else(|| fail(PolicyError::Overflow));
        payment_id
    }

    fn take_pending(&mut self, payment_id: u64) -> PendingPayment {
        let pending = self
            .pending
            .remove(&payment_id)
            .unwrap_or_else(|| fail_with(ERR_NOT_PENDING, "no payment is waiting with that id"));
        self.pending_order.retain(|id| *id != payment_id);
        pending
    }

    fn open_holds(&self, agent_id: &AccountId) -> usize {
        self.pending_order
            .iter()
            .filter(|id| {
                self.pending
                    .get(*id)
                    .is_some_and(|pending| &pending.agent_id == agent_id)
            })
            .count()
    }

    fn sweep_expired(&mut self) {
        let now = env::block_timestamp();
        let expired: Vec<u64> = self
            .pending_order
            .iter()
            .copied()
            .filter(|id| {
                self.pending
                    .get(id)
                    .is_some_and(|pending| now.saturating_sub(pending.created_at_ns) >= HOLD_TTL_NS)
            })
            .collect();
        for id in expired {
            let pending = self.take_pending(id);
            self.close_hold_slot(&pending.agent_id);
            WalletEvent::PaymentExpired {
                payment_id: U64(id),
                agent_id: pending.agent_id,
            }
            .emit();
        }
    }

    fn drop_hold(&mut self, payment_id: u64) {
        let pending = self.take_pending(payment_id);
        self.close_hold_slot(&pending.agent_id);
        WalletEvent::PaymentRejected {
            payment_id: U64(payment_id),
            agent_id: pending.agent_id,
            release: ReleaseStatus::Unreserved,
        }
        .emit();
    }

    fn close_hold_slot(&mut self, agent_id: &AccountId) {
        if let Some(stored) = self.agents.get_mut(agent_id) {
            let mut quota = stored.hold_quota();
            quota.close_one();
            stored.store_quota(quota);
        }
    }

    /// Budgets of the tokens on the allowlist. A budget kept for a removed
    /// token (it still carries that day's spend) is not shown.
    fn budget_views(&self, stored: &StoredAgent, now: u64) -> Vec<TokenBudgetView> {
        stored
            .budgets
            .iter()
            .filter(|budget| self.tokens.contains(&budget.token))
            .map(|budget| budget_view(stored, budget, now))
            .collect()
    }

    /// The budget of the wallet's only token, when it has exactly one.
    fn single_budget<'a>(&self, stored: &'a StoredAgent) -> Option<&'a TokenBudget> {
        match self.tokens.as_slice() {
            [token] => stored.budget(token),
            _ => None,
        }
    }

    fn schedule_transfer(transfer: ScheduledTransfer) -> Promise {
        let ScheduledTransfer {
            agent_id,
            token,
            receiver_id,
            amount,
            memo,
            payment_id,
            window_index,
        } = transfer;
        WalletEvent::PayRequested {
            payment_id: U64(payment_id),
            agent_id: agent_id.clone(),
            token: token.clone(),
            receiver_id: receiver_id.clone(),
            amount,
            window_index: U64(window_index),
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
                        U64(window_index),
                    ),
            )
    }

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

fn payee_names(payees: &[AccountId]) -> Vec<String> {
    payees.iter().map(ToString::to_string).collect()
}

fn empty_agent() -> StoredAgent {
    StoredAgent {
        payees: Vec::new(),
        active: true,
        holds_open: 0,
        holds_created: 0,
        holds_window: 0,
        budgets: Vec::new(),
    }
}

/// Built-in beta ceiling for a known mainnet token, if this id is one.
fn builtin_cap(token: &AccountId) -> Option<u128> {
    match token.as_str() {
        USDC_MAINNET => Some(BETA_USDC_CAP),
        WRAP_NEAR => Some(BETA_WNEAR_CAP),
        _ => None,
    }
}

fn initial_caps(tokens: &[AccountId]) -> Vec<TokenCap> {
    tokens
        .iter()
        .filter_map(|token| {
            builtin_cap(token).map(|max_balance| TokenCap {
                token: token.clone(),
                max_balance,
                custody: 0,
            })
        })
        .collect()
}

/// The policy that applies to `token`, or `None` when that token has no limits.
fn load_policy(stored: &StoredAgent, token: &AccountId) -> Option<AgentPolicy> {
    let budget = stored.budget(token)?;
    AgentPolicy::from_parts(
        budget.per_call_max,
        budget.daily_max,
        payee_names(&stored.payees),
        stored.active,
        budget.spent_in_window,
        budget.window_index,
    )
    .ok()
}

/// Write back the spend of a policy loaded by [`load_policy`]. Limits are
/// changed only by [`set_limits`].
fn write_spend(stored: &mut StoredAgent, token: &AccountId, policy: &AgentPolicy) {
    stored.active = policy.active();
    if let Some(budget) = stored.budgets.iter_mut().find(|b| &b.token == token) {
        budget.spent_in_window = policy.spent_in_window();
        budget.window_index = policy.window_index();
    }
}

/// Set `token`'s limits for one agent. The token's spend window is kept
/// (a lowered `daily_max` applies at once), and `active` is not touched.
/// When a new budget is added, budgets of tokens that left the allowlist
/// and carry no spend today are dropped, which bounds the list.
fn set_limits(
    stored: &mut StoredAgent,
    tokens: &[AccountId],
    token: &AccountId,
    per_call_max: u128,
    daily_max: u128,
    now: u64,
) -> Result<(), PolicyError> {
    // Validates per_call_max > 0, daily_max >= per_call_max and the payees.
    AgentPolicy::new(per_call_max, daily_max, payee_names(&stored.payees))?;
    if let Some(budget) = stored.budgets.iter_mut().find(|b| &b.token == token) {
        budget.per_call_max = per_call_max;
        budget.daily_max = daily_max;
        return Ok(());
    }
    let today = window_index(now);
    stored.budgets.retain(|b| {
        tokens.contains(&b.token) || (b.window_index >= today && b.spent_in_window > 0)
    });
    stored.budgets.push(TokenBudget {
        token: token.clone(),
        per_call_max,
        daily_max,
        spent_in_window: 0,
        window_index: 0,
    });
    Ok(())
}

fn holds_today(stored: &StoredAgent, now: u64) -> u32 {
    let window = window_index(now).max(stored.holds_window);
    if window > stored.holds_window {
        0
    } else {
        stored.holds_created
    }
}

fn budget_view(stored: &StoredAgent, budget: &TokenBudget, now: u64) -> TokenBudgetView {
    let policy = AgentPolicy::from_parts(
        budget.per_call_max,
        budget.daily_max,
        payee_names(&stored.payees),
        stored.active,
        budget.spent_in_window,
        budget.window_index,
    )
    .unwrap_or_else(|e| fail(e));
    TokenBudgetView {
        token: budget.token.clone(),
        per_call_max: U128(budget.per_call_max),
        daily_max: U128(budget.daily_max),
        spent_in_window: U128(policy.spent_at(now)),
        remaining: U128(policy.remaining_at(now)),
        window_index: U64(window_index(now).max(budget.window_index)),
    }
}
