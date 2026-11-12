//! Per-agent spend policy: limits, payee allowlist, and the spend window.

use crate::error::PolicyError;
use crate::window::window_index;

/// Maximum number of payees one agent policy may allowlist. Bounds the
/// storage an agent entry occupies and the gas a payee lookup costs.
pub const MAX_PAYEES: usize = 32;

/// Wallet-level switches that apply to every agent. The policy core does
/// not own them; the caller passes the current values in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalletGate {
    /// True when the owner has paused the wallet.
    pub paused: bool,
    /// True when the requested token is on the wallet's token allowlist.
    pub token_allowed: bool,
}

/// An amount held against an agent's window by [`AgentPolicy::check_and_reserve`].
///
/// The reservation remembers the window it was taken in, so a release that
/// arrives after the window rolled over does not subtract from the new
/// window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reservation {
    /// The reserved amount, in token atomic units.
    pub amount: u128,
    /// The UTC day the amount was reserved in.
    pub window_index: u64,
}

/// What a [`AgentPolicy::release`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// The amount was subtracted from the current window.
    Restored,
    /// The reservation's window has already rolled over; its spend was
    /// discarded with the old window, so nothing was subtracted.
    WindowExpired,
}

/// The spend policy of one agent.
///
/// Fields are private so every instance passes the same validation: build
/// one with [`AgentPolicy::new`] or restore one from storage with
/// [`AgentPolicy::from_parts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPolicy {
    per_call_max: u128,
    daily_max: u128,
    payees: Vec<String>,
    active: bool,
    spent_in_window: u128,
    window_index: u64,
}

fn validate_limits(
    per_call_max: u128,
    daily_max: u128,
    payees: &[String],
) -> Result<(), PolicyError> {
    if per_call_max == 0 || daily_max < per_call_max || payees.len() > MAX_PAYEES {
        return Err(PolicyError::InvalidPolicy);
    }
    Ok(())
}

fn normalize_payees(mut payees: Vec<String>) -> Vec<String> {
    payees.sort();
    payees.dedup();
    payees
}

impl AgentPolicy {
    /// A new, active policy with nothing spent.
    ///
    /// Payees are sorted and de-duplicated. Fails with
    /// [`PolicyError::InvalidPolicy`] when `per_call_max` is zero, when
    /// `daily_max < per_call_max`, or when more than [`MAX_PAYEES`] distinct
    /// payees are given.
    pub fn new(
        per_call_max: u128,
        daily_max: u128,
        payees: Vec<String>,
    ) -> Result<Self, PolicyError> {
        let payees = normalize_payees(payees);
        validate_limits(per_call_max, daily_max, &payees)?;
        Ok(AgentPolicy {
            per_call_max,
            daily_max,
            payees,
            active: true,
            spent_in_window: 0,
            window_index: 0,
        })
    }

    /// Restore a policy from stored parts, applying the same limit
    /// validation as [`AgentPolicy::new`]. `spent_in_window` may exceed
    /// `daily_max` after the owner lowered the limit; that state is legal
    /// and simply leaves no remaining budget.
    pub fn from_parts(
        per_call_max: u128,
        daily_max: u128,
        payees: Vec<String>,
        active: bool,
        spent_in_window: u128,
        window_index: u64,
    ) -> Result<Self, PolicyError> {
        let payees = normalize_payees(payees);
        validate_limits(per_call_max, daily_max, &payees)?;
        Ok(AgentPolicy {
            per_call_max,
            daily_max,
            payees,
            active,
            spent_in_window,
            window_index,
        })
    }

    /// Replace the limits and payees, keep the spend window, and set the
    /// agent active. A lowered `daily_max` applies at once: if the window
    /// already spent more, the remaining budget is zero until rollover.
    pub fn reconfigure(
        &mut self,
        per_call_max: u128,
        daily_max: u128,
        payees: Vec<String>,
    ) -> Result<(), PolicyError> {
        let payees = normalize_payees(payees);
        validate_limits(per_call_max, daily_max, &payees)?;
        self.per_call_max = per_call_max;
        self.daily_max = daily_max;
        self.payees = payees;
        self.active = true;
        Ok(())
    }

    /// Mark the agent revoked. The spend window is kept so a later
    /// re-activation inside the same day cannot reset the daily total.
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Largest single payment allowed.
    pub fn per_call_max(&self) -> u128 {
        self.per_call_max
    }

    /// Largest total allowed per UTC day.
    pub fn daily_max(&self) -> u128 {
        self.daily_max
    }

    /// The payee allowlist, sorted and de-duplicated.
    pub fn payees(&self) -> &[String] {
        &self.payees
    }

    /// False once the agent has been revoked.
    pub fn active(&self) -> bool {
        self.active
    }

    /// The stored window total, without applying rollover. Use
    /// [`AgentPolicy::spent_at`] for the value as of a given time.
    pub fn spent_in_window(&self) -> u128 {
        self.spent_in_window
    }

    /// The stored window's UTC day number.
    pub fn window_index(&self) -> u64 {
        self.window_index
    }

    /// True when `payee` is on the allowlist.
    pub fn is_payee_allowed(&self, payee: &str) -> bool {
        self.payees
            .binary_search_by(|p| p.as_str().cmp(payee))
            .is_ok()
    }

    /// The UTC day a payment at `now_ns` is counted in. Time never moves
    /// backwards on chain; if it appears to, the stored window keeps
    /// counting (fail closed) instead of resetting.
    fn effective_window(&self, now_ns: u64) -> u64 {
        window_index(now_ns).max(self.window_index)
    }

    /// The window total as of `now_ns`: zero once the stored window is in
    /// the past.
    pub fn spent_at(&self, now_ns: u64) -> u128 {
        if self.effective_window(now_ns) > self.window_index {
            0
        } else {
            self.spent_in_window
        }
    }

    /// Budget left in the window as of `now_ns`, saturating at zero.
    pub fn remaining_at(&self, now_ns: u64) -> u128 {
        self.daily_max.saturating_sub(self.spent_at(now_ns))
    }

    /// Advance the stored window to the day containing `now_ns`, resetting
    /// the total when the day changed. A no-op inside the same day and when
    /// `now_ns` is earlier than the stored window.
    pub fn rollover(&mut self, now_ns: u64) {
        let window = self.effective_window(now_ns);
        if window > self.window_index {
            self.window_index = window;
            self.spent_in_window = 0;
        }
    }

    /// Check a payment and, when every check passes, reserve its amount.
    ///
    /// Checks run in a fixed order, and the first failure is returned:
    /// paused, agent inactive, token not allowed, payee not allowed, zero
    /// amount, per-call limit, daily limit. On any failure the policy is
    /// left exactly as it was, including its window.
    pub fn check_and_reserve(
        &mut self,
        gate: WalletGate,
        payee: &str,
        amount: u128,
        now_ns: u64,
    ) -> Result<Reservation, PolicyError> {
        self.reserve(gate, payee, amount, now_ns, false)
    }

    /// Same checks and reservation as [`check_and_reserve`], except the
    /// payee allowlist. The wallet uses this when the owner has asked to
    /// hold an unknown payee for approval. The amount still counts against
    /// the per-call and daily limits, and a failure still leaves the policy
    /// unchanged.
    pub fn check_and_reserve_unlisted(
        &mut self,
        gate: WalletGate,
        amount: u128,
        now_ns: u64,
    ) -> Result<Reservation, PolicyError> {
        self.reserve(gate, "", amount, now_ns, true)
    }

    fn reserve(
        &mut self,
        gate: WalletGate,
        payee: &str,
        amount: u128,
        now_ns: u64,
        skip_allowlist: bool,
    ) -> Result<Reservation, PolicyError> {
        if gate.paused {
            return Err(PolicyError::Paused);
        }
        if !self.active {
            return Err(PolicyError::AgentInactive);
        }
        if !gate.token_allowed {
            return Err(PolicyError::TokenNotAllowed);
        }
        if !skip_allowlist && !self.is_payee_allowed(payee) {
            return Err(PolicyError::PayeeNotAllowed);
        }
        if amount == 0 {
            return Err(PolicyError::ZeroAmount);
        }
        if amount > self.per_call_max {
            return Err(PolicyError::PerCallLimit);
        }
        let window = self.effective_window(now_ns);
        let spent = self.spent_at(now_ns);
        let total = spent.checked_add(amount).ok_or(PolicyError::Overflow)?;
        if total > self.daily_max {
            return Err(PolicyError::DailyLimit);
        }
        self.window_index = window;
        self.spent_in_window = total;
        Ok(Reservation {
            amount,
            window_index: window,
        })
    }

    /// Return a reservation, for example after the downstream transfer
    /// failed.
    ///
    /// When the reservation's window is still the stored window, its amount
    /// is subtracted with checked math; [`PolicyError::ReleaseMismatch`]
    /// means the window holds less than the reservation and nothing
    /// changed. When the window has already rolled over, nothing is
    /// subtracted and [`ReleaseOutcome::WindowExpired`] is returned.
    pub fn release(&mut self, reservation: Reservation) -> Result<ReleaseOutcome, PolicyError> {
        if reservation.window_index < self.window_index {
            return Ok(ReleaseOutcome::WindowExpired);
        }
        if reservation.window_index > self.window_index {
            return Err(PolicyError::ReleaseMismatch);
        }
        self.spent_in_window = self
            .spent_in_window
            .checked_sub(reservation.amount)
            .ok_or(PolicyError::ReleaseMismatch)?;
        Ok(ReleaseOutcome::Restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::NANOS_PER_DAY;

    const OPEN: WalletGate = WalletGate {
        paused: false,
        token_allowed: true,
    };
    const DAY5: u64 = 5 * NANOS_PER_DAY + 123;

    fn policy() -> AgentPolicy {
        AgentPolicy::new(50_000, 200_000, vec!["gateway.near".to_string()]).unwrap()
    }

    #[test]
    fn new_policy_is_active_and_empty() {
        let p = policy();
        assert!(p.active());
        assert_eq!(p.spent_in_window(), 0);
        assert_eq!(p.per_call_max(), 50_000);
        assert_eq!(p.daily_max(), 200_000);
    }

    #[test]
    fn invalid_limits_are_rejected() {
        assert_eq!(
            AgentPolicy::new(0, 10, vec![]),
            Err(PolicyError::InvalidPolicy)
        );
        assert_eq!(
            AgentPolicy::new(11, 10, vec![]),
            Err(PolicyError::InvalidPolicy)
        );
        assert!(AgentPolicy::new(10, 10, vec![]).is_ok());
    }

    #[test]
    fn too_many_payees_are_rejected_after_dedup() {
        let many: Vec<String> = (0..=MAX_PAYEES).map(|i| format!("p{i}.near")).collect();
        assert_eq!(
            AgentPolicy::new(1, 1, many),
            Err(PolicyError::InvalidPolicy)
        );
        let dupes: Vec<String> = (0..100).map(|_| "same.near".to_string()).collect();
        let p = AgentPolicy::new(1, 1, dupes).unwrap();
        assert_eq!(p.payees(), ["same.near".to_string()]);
    }

    #[test]
    fn reserve_within_limits_succeeds() {
        let mut p = policy();
        let r = p
            .check_and_reserve(OPEN, "gateway.near", 50_000, DAY5)
            .unwrap();
        assert_eq!(
            r,
            Reservation {
                amount: 50_000,
                window_index: 5
            }
        );
        assert_eq!(p.spent_in_window(), 50_000);
        assert_eq!(p.window_index(), 5);
        assert_eq!(p.remaining_at(DAY5), 150_000);
    }

    #[test]
    fn unlisted_reserve_skips_the_allowlist_and_keeps_the_limits() {
        let mut p = policy();
        let reserved = p.check_and_reserve_unlisted(OPEN, 50_000, DAY5).unwrap();
        assert_eq!(reserved.amount, 50_000);
        assert_eq!(p.spent_in_window(), 50_000);
        assert!(!p.is_payee_allowed("stranger.near"));
        assert_eq!(
            p.check_and_reserve_unlisted(OPEN, 50_001, DAY5),
            Err(PolicyError::PerCallLimit)
        );
        assert_eq!(p.spent_in_window(), 50_000);
    }

    #[test]
    fn per_call_limit_is_enforced() {
        let mut p = policy();
        assert_eq!(
            p.check_and_reserve(OPEN, "gateway.near", 50_001, DAY5),
            Err(PolicyError::PerCallLimit)
        );
        assert_eq!(p, policy());
    }

    #[test]
    fn daily_limit_is_enforced_at_the_boundary() {
        let mut p = policy();
        for _ in 0..4 {
            p.check_and_reserve(OPEN, "gateway.near", 50_000, DAY5)
                .unwrap();
        }
        assert_eq!(p.remaining_at(DAY5), 0);
        let before = p.clone();
        assert_eq!(
            p.check_and_reserve(OPEN, "gateway.near", 1, DAY5),
            Err(PolicyError::DailyLimit)
        );
        assert_eq!(p, before);
    }

    #[test]
    fn payee_allowlist_is_enforced() {
        let mut p = policy();
        assert_eq!(
            p.check_and_reserve(OPEN, "attacker.near", 1, DAY5),
            Err(PolicyError::PayeeNotAllowed)
        );
        // Prefix and case variants are different accounts.
        assert!(!p.is_payee_allowed("gateway.nea"));
        assert!(!p.is_payee_allowed("GATEWAY.near"));
    }

    #[test]
    fn token_allowlist_is_enforced() {
        let mut p = policy();
        let gate = WalletGate {
            paused: false,
            token_allowed: false,
        };
        assert_eq!(
            p.check_and_reserve(gate, "gateway.near", 1, DAY5),
            Err(PolicyError::TokenNotAllowed)
        );
    }

    #[test]
    fn paused_wallet_refuses_first() {
        let mut p = policy();
        p.deactivate();
        let gate = WalletGate {
            paused: true,
            token_allowed: false,
        };
        // Paused wins over every other failure.
        assert_eq!(
            p.check_and_reserve(gate, "nobody.near", 0, DAY5),
            Err(PolicyError::Paused)
        );
    }

    #[test]
    fn inactive_agent_is_refused() {
        let mut p = policy();
        p.deactivate();
        assert_eq!(
            p.check_and_reserve(OPEN, "gateway.near", 1, DAY5),
            Err(PolicyError::AgentInactive)
        );
    }

    #[test]
    fn zero_amount_is_refused() {
        let mut p = policy();
        assert_eq!(
            p.check_and_reserve(OPEN, "gateway.near", 0, DAY5),
            Err(PolicyError::ZeroAmount)
        );
    }

    #[test]
    fn rollover_resets_the_window() {
        let mut p = policy();
        for _ in 0..4 {
            p.check_and_reserve(OPEN, "gateway.near", 50_000, DAY5)
                .unwrap();
        }
        let next_day = 6 * NANOS_PER_DAY;
        assert_eq!(p.spent_at(next_day), 0);
        assert_eq!(p.remaining_at(next_day), 200_000);
        let r = p
            .check_and_reserve(OPEN, "gateway.near", 50_000, next_day)
            .unwrap();
        assert_eq!(r.window_index, 6);
        assert_eq!(p.spent_in_window(), 50_000);
    }

    #[test]
    fn last_nanosecond_of_the_day_is_the_same_window() {
        let mut p = policy();
        for _ in 0..4 {
            p.check_and_reserve(OPEN, "gateway.near", 50_000, 5 * NANOS_PER_DAY)
                .unwrap();
        }
        assert_eq!(
            p.check_and_reserve(OPEN, "gateway.near", 1, 6 * NANOS_PER_DAY - 1),
            Err(PolicyError::DailyLimit)
        );
    }

    #[test]
    fn backwards_clock_does_not_reset() {
        let mut p = policy();
        p.check_and_reserve(OPEN, "gateway.near", 50_000, DAY5)
            .unwrap();
        p.rollover(NANOS_PER_DAY);
        assert_eq!(p.window_index(), 5);
        assert_eq!(p.spent_in_window(), 50_000);
        assert_eq!(p.spent_at(NANOS_PER_DAY), 50_000);
    }

    #[test]
    fn explicit_rollover_is_idempotent_within_a_day() {
        let mut p = policy();
        p.check_and_reserve(OPEN, "gateway.near", 10, DAY5).unwrap();
        p.rollover(DAY5 + 1);
        assert_eq!(p.spent_in_window(), 10);
        p.rollover(7 * NANOS_PER_DAY);
        assert_eq!((p.window_index(), p.spent_in_window()), (7, 0));
        p.rollover(7 * NANOS_PER_DAY + 5);
        assert_eq!((p.window_index(), p.spent_in_window()), (7, 0));
    }

    #[test]
    fn release_restores_the_window() {
        let mut p = policy();
        p.check_and_reserve(OPEN, "gateway.near", 20_000, DAY5)
            .unwrap();
        let before = p.clone();
        let r = p
            .check_and_reserve(OPEN, "gateway.near", 30_000, DAY5)
            .unwrap();
        assert_eq!(p.release(r), Ok(ReleaseOutcome::Restored));
        assert_eq!(p, before);
    }

    #[test]
    fn release_after_rollover_is_a_noop() {
        let mut p = policy();
        let r = p
            .check_and_reserve(OPEN, "gateway.near", 30_000, DAY5)
            .unwrap();
        p.check_and_reserve(OPEN, "gateway.near", 5_000, 6 * NANOS_PER_DAY)
            .unwrap();
        assert_eq!(p.release(r), Ok(ReleaseOutcome::WindowExpired));
        assert_eq!(p.spent_in_window(), 5_000);
    }

    #[test]
    fn release_larger_than_window_is_refused() {
        let mut p = policy();
        p.check_and_reserve(OPEN, "gateway.near", 10, DAY5).unwrap();
        let bogus = Reservation {
            amount: 11,
            window_index: 5,
        };
        assert_eq!(p.release(bogus), Err(PolicyError::ReleaseMismatch));
        assert_eq!(p.spent_in_window(), 10);
        let future = Reservation {
            amount: 1,
            window_index: 9,
        };
        assert_eq!(p.release(future), Err(PolicyError::ReleaseMismatch));
    }

    #[test]
    fn overflow_adjacent_amounts_use_checked_math() {
        let mut p = AgentPolicy::from_parts(
            u128::MAX,
            u128::MAX,
            vec!["g".into()],
            true,
            u128::MAX - 1,
            5,
        )
        .unwrap();
        assert_eq!(
            p.check_and_reserve(OPEN, "g", 2, DAY5),
            Err(PolicyError::Overflow)
        );
        let r = p.check_and_reserve(OPEN, "g", 1, DAY5).unwrap();
        assert_eq!(p.spent_in_window(), u128::MAX);
        assert_eq!(p.release(r), Ok(ReleaseOutcome::Restored));
        assert_eq!(p.spent_in_window(), u128::MAX - 1);
    }

    #[test]
    fn reconfigure_keeps_spend_and_reactivates() {
        let mut p = policy();
        p.check_and_reserve(OPEN, "gateway.near", 50_000, DAY5)
            .unwrap();
        p.deactivate();
        p.reconfigure(10_000, 40_000, vec!["other.near".into()])
            .unwrap();
        assert!(p.active());
        assert_eq!(p.spent_in_window(), 50_000);
        assert_eq!(p.remaining_at(DAY5), 0);
        assert_eq!(
            p.check_and_reserve(OPEN, "other.near", 1, DAY5),
            Err(PolicyError::DailyLimit)
        );
        assert_eq!(p.reconfigure(0, 1, vec![]), Err(PolicyError::InvalidPolicy));
    }

    #[test]
    fn error_codes_are_stable_prefixes() {
        let all = [
            (PolicyError::DailyLimit, "ERR_DAILY_LIMIT"),
            (PolicyError::PerCallLimit, "ERR_PER_CALL_LIMIT"),
            (PolicyError::PayeeNotAllowed, "ERR_PAYEE_NOT_ALLOWED"),
            (PolicyError::TokenNotAllowed, "ERR_TOKEN_NOT_ALLOWED"),
            (PolicyError::Paused, "ERR_PAUSED"),
            (PolicyError::AgentInactive, "ERR_AGENT_INACTIVE"),
            (PolicyError::ZeroAmount, "ERR_ZERO_AMOUNT"),
            (PolicyError::InvalidPolicy, "ERR_INVALID_POLICY"),
            (PolicyError::Overflow, "ERR_OVERFLOW"),
            (PolicyError::ReleaseMismatch, "ERR_RELEASE_MISMATCH"),
        ];
        for (err, code) in all {
            assert_eq!(err.code(), code);
            assert!(err.to_string().starts_with(&format!("{code}: ")));
        }
    }
}
