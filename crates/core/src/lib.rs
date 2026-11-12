//! TriagePay core: a prepaid, idempotent payment ledger for metered
//! agent-tool calls.
//!
//! Design rules enforced in this crate:
//! - integer money only, u128 atomic units, no floats;
//! - every money-moving operation carries an idempotency key scoped to the
//!   caller: the same key with the same payload replays the recorded
//!   result, the same key with a changed payload is a hard error;
//! - credit is reserved before work starts, settled only against a
//!   non-zero report hash, and refunded on failure;
//! - the ledger conserves credit: deposited_total equals available plus
//!   reserved plus settled_total for every account, at all times.

pub mod account;
pub mod error;
pub mod ledger;
pub mod money;
pub mod pricebook;
pub mod state;

pub use account::Balances;
pub use error::CoreError;
pub use ledger::{DepositReceipt, JobSnapshot, Ledger, RefundReceipt, Reservation, Settlement};
pub use money::{parse_decimal, Amount, SETTLEMENT_DECIMALS};
pub use pricebook::{PriceBook, SCAN_SKU, TRIAGE_V2_SKU};
pub use state::{JobId, JobState, ReportHash};
