//! NEP-297 events emitted by the policy wallet.
//!
//! Every event is logged as
//! `EVENT_JSON:{"standard":"aps_policy_wallet","version":"1.0.0","event":"<name>","data":{...}}`.

use near_sdk::json_types::{U128, U64};
use near_sdk::{near, AccountId};

/// The NEP-297 `standard` field of every wallet event.
pub const EVENT_STANDARD: &str = "aps_policy_wallet";
/// The NEP-297 `version` field of every wallet event.
pub const EVENT_VERSION: &str = "1.0.0";

/// What happened to a reservation after a failed transfer.
#[near(serializers = [json])]
#[serde(rename_all = "snake_case")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseStatus {
    /// The amount was returned to the agent's window.
    Restored,
    /// The window had already rolled over; nothing needed returning.
    WindowExpired,
    /// The agent entry no longer holds the reservation (should not occur;
    /// logged instead of panicking so the callback never reverts).
    Mismatch,
}

/// Wallet events. Variant names serialize as snake_case event names.
#[near(event_json(standard = "aps_policy_wallet"))]
pub enum WalletEvent {
    /// An agent's payment passed policy and its transfer was scheduled.
    #[event_version("1.0.0")]
    PayRequested {
        /// Monotonic id of this payment within the wallet.
        payment_id: U64,
        /// The paying agent (the `pay` predecessor).
        agent_id: AccountId,
        /// The NEP-141 token contract.
        token: AccountId,
        /// The payee.
        receiver_id: AccountId,
        /// Amount in token atomic units.
        amount: U128,
        /// UTC day the amount was reserved in.
        window_index: U64,
    },
    /// The `ft_transfer` for a payment succeeded.
    #[event_version("1.0.0")]
    PaySettled {
        /// Payment id from `pay_requested`.
        payment_id: U64,
        /// The paying agent.
        agent_id: AccountId,
        /// The NEP-141 token contract.
        token: AccountId,
        /// The payee.
        receiver_id: AccountId,
        /// Amount in token atomic units.
        amount: U128,
    },
    /// The `ft_transfer` for a payment failed; the reservation was handled
    /// as `release`.
    #[event_version("1.0.0")]
    PayFailed {
        /// Payment id from `pay_requested`.
        payment_id: U64,
        /// The paying agent.
        agent_id: AccountId,
        /// The NEP-141 token contract.
        token: AccountId,
        /// The payee.
        receiver_id: AccountId,
        /// Amount in token atomic units.
        amount: U128,
        /// What happened to the reservation.
        release: ReleaseStatus,
    },
    /// The owner created or reconfigured an agent.
    #[event_version("1.0.0")]
    AgentSet {
        /// The agent account.
        agent_id: AccountId,
        /// Per-call limit in token atomic units.
        per_call_max: U128,
        /// Daily limit in token atomic units.
        daily_max: U128,
        /// Payee allowlist, sorted.
        payees: Vec<AccountId>,
    },
    /// The owner revoked an agent.
    #[event_version("1.0.0")]
    AgentRevoked {
        /// The agent account.
        agent_id: AccountId,
    },
    /// The owner replaced the token allowlist.
    #[event_version("1.0.0")]
    TokensSet {
        /// The new allowlist, sorted.
        tokens: Vec<AccountId>,
    },
    /// The owner paused the wallet.
    #[event_version("1.0.0")]
    Paused {
        /// The owner account.
        by: AccountId,
    },
    /// The owner unpaused the wallet.
    #[event_version("1.0.0")]
    Unpaused {
        /// The owner account.
        by: AccountId,
    },
    /// An allowlisted token funded the wallet through `ft_transfer_call`.
    #[event_version("1.0.0")]
    Funded {
        /// The NEP-141 token contract.
        token: AccountId,
        /// The account that sent the tokens.
        sender_id: AccountId,
        /// Amount in token atomic units.
        amount: U128,
    },
    /// An owner withdrawal transfer succeeded.
    #[event_version("1.0.0")]
    Withdrawn {
        /// The NEP-141 token contract.
        token: AccountId,
        /// Amount in token atomic units.
        amount: U128,
        /// Destination account.
        to: AccountId,
    },
    /// An owner withdrawal transfer failed; no tokens left the wallet.
    #[event_version("1.0.0")]
    WithdrawFailed {
        /// The NEP-141 token contract.
        token: AccountId,
        /// Amount in token atomic units.
        amount: U128,
        /// Destination account.
        to: AccountId,
    },
}
