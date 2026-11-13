//! Pure fee accounting for `create_wallet`.
//!
//! The contract uses these functions so a property test can state the
//! invariants without a chain:
//! - a short deposit is returned in full by the runtime and pays no fee;
//! - the excess over fee + storage is always refunded;
//! - on success the fee reaches `fee_recipient` and the storage stays on
//!   the new wallet;
//! - on a failed creation the user is refunded the whole attachment and
//!   `fee_recipient` receives nothing.

/// Why a deposit was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteError {
    /// `fee + storage` overflowed u128.
    Overflow,
    /// The attachment was below fee + storage.
    Short,
}

/// Where an accepted deposit goes. Every field is in yoctoNEAR.
/// The five amounts sum to the attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flows {
    /// Sent back to the user before the create promise.
    pub excess_refund: u128,
    /// Locked on the new wallet. Zero when creation fails.
    pub storage_locked: u128,
    /// Sent to `fee_recipient`. Zero unless creation succeeds.
    pub fee_to_recipient: u128,
    /// Sent back to the user from the callback when creation fails.
    pub callback_refund: u128,
    /// Returned by a panic, before any promise. The whole attachment.
    pub runtime_refund: u128,
}

impl Flows {
    /// Everything that comes back to the user.
    pub fn user_refund(self) -> u128 {
        self.excess_refund
            .saturating_add(self.callback_refund)
            .saturating_add(self.runtime_refund)
    }

    /// True when the five flows add up to `attached` with no overflow.
    pub fn conserves(self, attached: u128) -> bool {
        let mut sum = 0u128;
        for part in [
            self.excess_refund,
            self.storage_locked,
            self.fee_to_recipient,
            self.callback_refund,
            self.runtime_refund,
        ] {
            sum = match sum.checked_add(part) {
                Some(n) => n,
                None => return false,
            };
        }
        sum == attached
    }
}

/// Fee plus storage, or [`QuoteError::Overflow`].
pub fn required_deposit(fee: u128, storage: u128) -> Result<u128, QuoteError> {
    fee.checked_add(storage).ok_or(QuoteError::Overflow)
}

/// Split `attached` once creation has either succeeded or failed.
///
/// `Ok` is returned for a short deposit too: that case is a full runtime
/// refund, which is still a complete accounting. `Err(Overflow)` is the
/// only failure, and the contract treats it like a short deposit (panic,
/// runtime refunds the attachment).
pub fn settle(attached: u128, fee: u128, storage: u128, created: bool) -> Result<Flows, QuoteError> {
    let cost = match required_deposit(fee, storage) {
        Ok(cost) => cost,
        Err(QuoteError::Overflow) => return Err(QuoteError::Overflow),
        Err(QuoteError::Short) => unreachable!("required_deposit does not return Short"),
    };
    if attached < cost {
        return Ok(Flows {
            excess_refund: 0,
            storage_locked: 0,
            fee_to_recipient: 0,
            callback_refund: 0,
            runtime_refund: attached,
        });
    }
    let excess = attached - cost;
    if created {
        Ok(Flows {
            excess_refund: excess,
            storage_locked: storage,
            fee_to_recipient: fee,
            callback_refund: 0,
            runtime_refund: 0,
        })
    } else {
        Ok(Flows {
            excess_refund: excess,
            storage_locked: 0,
            fee_to_recipient: 0,
            callback_refund: cost,
            runtime_refund: 0,
        })
    }
}

/// A wallet subaccount name. Two to 32 bytes, lowercase letters, digits,
/// hyphen and underscore, not starting or ending with a separator. The full
/// account id, `name.parent`, must fit in 64 bytes.
pub fn wallet_account_id(parent: &str, name: &str) -> Result<String, &'static str> {
    if !(2..=32).contains(&name.len()) {
        return Err("name length");
    }
    let bytes = name.as_bytes();
    if !bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-' || *b == b'_')
    {
        return Err("name characters");
    }
    if bytes[0] == b'-' || bytes[0] == b'_' || bytes[bytes.len() - 1] == b'-' || bytes[bytes.len() - 1] == b'_'
    {
        return Err("name separator");
    }
    if parent.is_empty() || parent.len() + 1 + name.len() > 64 {
        return Err("account length");
    }
    if parent.contains(' ') {
        return Err("parent");
    }
    Ok(format!("{name}.{parent}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Whatever the amounts and the outcome, the fee reaches the
        /// recipient only when the wallet is created, the excess always
        /// comes back, a failed creation refunds the user in full, and
        /// nothing is created or destroyed.
        #[test]
        fn fee_excess_and_failure_refunds_are_exact(
            fee in 0u128..=u128::MAX / 4,
            storage in 0u128..=u128::MAX / 4,
            extra in 0u128..=u128::MAX / 4,
            shortfall in 1u128..=1_000_000_000_000_000_000,
            created in any::<bool>(),
        ) {
            let cost = fee + storage;
            let attached_ok = cost + extra;
            let flows = settle(attached_ok, fee, storage, created).unwrap();
            prop_assert!(flows.conserves(attached_ok));
            prop_assert_eq!(flows.runtime_refund, 0);
            prop_assert_eq!(flows.excess_refund, extra);
            if created {
                prop_assert_eq!(flows.fee_to_recipient, fee);
                prop_assert_eq!(flows.storage_locked, storage);
                prop_assert_eq!(flows.callback_refund, 0);
                prop_assert_eq!(flows.user_refund(), extra);
            } else {
                prop_assert_eq!(flows.fee_to_recipient, 0);
                prop_assert_eq!(flows.storage_locked, 0);
                prop_assert_eq!(flows.callback_refund, cost);
                prop_assert_eq!(flows.user_refund(), attached_ok);
            }

            let short = cost.saturating_sub(shortfall);
            if short < cost {
                let flows = settle(short, fee, storage, created).unwrap();
                prop_assert!(flows.conserves(short));
                prop_assert_eq!(flows.fee_to_recipient, 0);
                prop_assert_eq!(flows.storage_locked, 0);
                prop_assert_eq!(flows.excess_refund, 0);
                prop_assert_eq!(flows.callback_refund, 0);
                prop_assert_eq!(flows.user_refund(), short);
            }
        }

        #[test]
        fn overflow_is_refused(fee in (u128::MAX / 2)..=u128::MAX, storage in (u128::MAX / 2)..=u128::MAX) {
            prop_assume!(fee.checked_add(storage).is_none());
            prop_assert_eq!(required_deposit(fee, storage), Err(QuoteError::Overflow));
            prop_assert!(settle(u128::MAX, fee, storage, true).is_err());
        }
    }

    proptest! {
        #[test]
        fn valid_names_round_trip(body in "[a-z0-9][a-z0-9]{0,30}") {
            let name = body;
            prop_assume!(name.len() >= 2);
            let id = wallet_account_id("triagepay.hlpok1lad.tg", &name).unwrap();
            prop_assert!(id.ends_with(".triagepay.hlpok1lad.tg"));
            prop_assert!(id.len() <= 64);
            prop_assert_eq!(id, format!("{name}.triagepay.hlpok1lad.tg"));
        }

        #[test]
        fn garbage_names_are_rejected(name in "\\PC{0,40}") {
            let ok = wallet_account_id("triagepay.hlpok1lad.tg", &name);
            let bytes_ok = (2..=32).contains(&name.len())
                && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
                && !name.starts_with('-')
                && !name.starts_with('_')
                && !name.ends_with('-')
                && !name.ends_with('_');
            prop_assert_eq!(ok.is_ok(), bytes_ok);
        }
    }

    #[test]
    fn the_documented_fee_and_a_zero_storage_still_conserve() {
        let fee = 1_000_000_000_000_000_000_000_000;
        let flows = settle(fee + 5, fee, 0, true).unwrap();
        assert_eq!(flows.fee_to_recipient, fee);
        assert_eq!(flows.excess_refund, 5);
        assert!(flows.conserves(fee + 5));
        let failed = settle(fee + 5, fee, 0, false).unwrap();
        assert_eq!(failed.user_refund(), fee + 5);
        assert_eq!(failed.fee_to_recipient, 0);
    }
}
