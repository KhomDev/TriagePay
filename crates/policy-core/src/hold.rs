//! Per-agent cap on open holds and on how many holds one UTC day may create.
//!
//! A hold does not reserve spend. This ledger only counts queue slots, so
//! one agent cannot fill the owner's review queue or refresh that fill all
//! day by opening a new hold every time one is rejected.

use crate::error::PolicyError;
use crate::window::window_index;

/// Open holds one agent may have at once.
pub const MAX_OPEN_HOLDS_PER_AGENT: u32 = 3;
/// Holds one agent may open in one UTC day, including ones already closed.
pub const MAX_DAILY_HOLDS_PER_AGENT: u32 = 6;

/// Queue-slot counters for one agent. Fields stay private so every change
/// goes through [`HoldQuota::try_open`] or [`HoldQuota::close_one`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldQuota {
    open: u32,
    created_in_window: u32,
    window_index: u64,
}

impl Default for HoldQuota {
    fn default() -> Self {
        Self::new()
    }
}

impl HoldQuota {
    /// Nothing open and nothing created.
    pub const fn new() -> Self {
        Self {
            open: 0,
            created_in_window: 0,
            window_index: 0,
        }
    }

    /// Restore counters from storage.
    pub const fn from_parts(open: u32, created_in_window: u32, window_index: u64) -> Self {
        Self {
            open,
            created_in_window,
            window_index,
        }
    }

    /// Holds currently occupying a queue slot.
    pub const fn open(self) -> u32 {
        self.open
    }

    /// Holds opened in the stored UTC day.
    pub const fn created_in_window(self) -> u32 {
        self.created_in_window
    }

    /// UTC day the creation counter belongs to.
    pub const fn window_index(self) -> u64 {
        self.window_index
    }

    /// Reserve one queue slot as of `now_ns`.
    ///
    /// On failure the quota is unchanged, including its window. A new UTC
    /// day resets the creation counter before the daily check, so yesterday's
    /// quota does not block today. The open-hold cap does not reset at
    /// midnight: a slot stays occupied until the hold is closed.
    pub fn try_open(&mut self, now_ns: u64) -> Result<(), PolicyError> {
        let window = window_index(now_ns).max(self.window_index);
        let created = if window > self.window_index {
            0
        } else {
            self.created_in_window
        };
        if self.open >= MAX_OPEN_HOLDS_PER_AGENT {
            return Err(PolicyError::AgentOpenHolds);
        }
        if created >= MAX_DAILY_HOLDS_PER_AGENT {
            return Err(PolicyError::AgentDailyHolds);
        }
        self.window_index = window;
        self.created_in_window = created + 1;
        self.open += 1;
        Ok(())
    }

    /// Free one queue slot. Creation count is kept, so rejecting a hold
    /// does not restore the day's quota. Extra closes saturate at zero.
    pub fn close_one(&mut self) {
        self.open = self.open.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::NANOS_PER_DAY;

    #[test]
    fn open_cap_is_three_and_does_not_change_the_quota_on_failure() {
        let mut q = HoldQuota::new();
        for _ in 0..3 {
            q.try_open(NANOS_PER_DAY).unwrap();
        }
        let before = q;
        assert_eq!(q.try_open(NANOS_PER_DAY), Err(PolicyError::AgentOpenHolds));
        assert_eq!(q, before);
        assert_eq!(q.open(), 3);
    }

    #[test]
    fn closing_frees_a_slot_but_not_the_daily_quota() {
        let mut q = HoldQuota::new();
        for _ in 0..6 {
            q.try_open(5 * NANOS_PER_DAY).unwrap();
            q.close_one();
        }
        assert_eq!(q.open(), 0);
        assert_eq!(q.created_in_window(), 6);
        assert_eq!(
            q.try_open(5 * NANOS_PER_DAY),
            Err(PolicyError::AgentDailyHolds)
        );
        assert_eq!(q.created_in_window(), 6);
    }

    #[test]
    fn a_new_utc_day_resets_the_creation_quota_only() {
        let mut q = HoldQuota::new();
        for _ in 0..3 {
            q.try_open(2 * NANOS_PER_DAY).unwrap();
        }
        assert_eq!(
            q.try_open(3 * NANOS_PER_DAY),
            Err(PolicyError::AgentOpenHolds)
        );
        q.close_one();
        q.try_open(3 * NANOS_PER_DAY).unwrap();
        assert_eq!(q.created_in_window(), 1);
        assert_eq!(q.window_index(), 3);
        assert_eq!(q.open(), 3);
    }
}
