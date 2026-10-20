//! Account balances for the prepaid ledger.
//!
//! Conservation invariant: for every account at all times,
//! deposited_total equals available plus reserved plus settled_total.
//! Refunds move credit from reserved back to available and only bump the
//! refunded_total lifetime counter.

use crate::money::Amount;

/// Balance snapshot of one prepaid account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Balances {
    /// Credit currently spendable.
    pub available: Amount,
    /// Credit locked by open reservations.
    pub reserved: Amount,
    /// Lifetime credit consumed by settled jobs.
    pub settled_total: Amount,
    /// Lifetime credit returned by refunds.
    pub refunded_total: Amount,
    /// Lifetime sum of successful deposits.
    pub deposited_total: Amount,
}

impl Balances {
    /// Verify the conservation invariant for this account.
    pub fn is_conserved(&self) -> bool {
        match self
            .available
            .checked_add(self.reserved)
            .and_then(|sum| sum.checked_add(self.settled_total))
        {
            Ok(active) => active == self.deposited_total,
            Err(_) => false,
        }
    }
}
