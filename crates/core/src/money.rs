//! Integer money. No floats, ever.
//!
//! All amounts are u128 atomic units. The default settlement asset uses six
//! decimals; parsing and formatting always carry the decimal width so a
//! six-decimal amount can never silently masquerade as an eight-decimal one.

use crate::error::CoreError;
use std::str::FromStr;

/// Decimals of the settlement asset in the default configuration.
pub const SETTLEMENT_DECIMALS: u32 = 6;

/// An amount of money in atomic integer units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Amount(u128);

impl Amount {
    /// The zero amount.
    pub const ZERO: Amount = Amount(0);

    /// Construct from raw atomic units.
    pub const fn from_atomic(units: u128) -> Self {
        Amount(units)
    }

    /// Construct from whole units at the given decimal width.
    pub fn from_whole_units(whole: u64, decimals: u32) -> Result<Self, CoreError> {
        let scale = 10u128.checked_pow(decimals).ok_or(CoreError::Overflow)?;
        let units = u128::from(whole)
            .checked_mul(scale)
            .ok_or(CoreError::Overflow)?;
        Ok(Amount(units))
    }

    /// Raw atomic units.
    pub const fn atomic(self) -> u128 {
        self.0
    }

    /// Checked addition.
    pub fn checked_add(self, rhs: Amount) -> Result<Amount, CoreError> {
        self.0
            .checked_add(rhs.0)
            .map(Amount)
            .ok_or(CoreError::Overflow)
    }

    /// Checked subtraction. Underflow is reported as Overflow; the ledger
    /// checks funds sufficiency before subtracting.
    pub fn checked_sub(self, rhs: Amount) -> Result<Amount, CoreError> {
        self.0
            .checked_sub(rhs.0)
            .map(Amount)
            .ok_or(CoreError::Overflow)
    }

    /// True when the amount is exactly zero.
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Format as a plain decimal string, for example 1.500000 at six
    /// decimals. Display helper only: keep atomic units on all hot paths.
    pub fn format(self, decimals: u32) -> String {
        let factor = 10u128.pow(decimals);
        let whole = self.0 / factor;
        let frac = self.0 % factor;
        if decimals == 0 {
            return whole.to_string();
        }
        let frac_str = frac.to_string();
        let pad = (decimals as usize).saturating_sub(frac_str.len());
        let mut out = whole.to_string();
        out.push('.');
        for _ in 0..pad {
            out.push('0');
        }
        out.push_str(&frac_str);
        out
    }
}

impl FromStr for Amount {
    type Err = CoreError;

    /// Parse a non-negative decimal string at settlement decimals.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_decimal(s, SETTLEMENT_DECIMALS)
    }
}

/// Parse a non-negative decimal string at an explicit decimal width.
pub fn parse_decimal(s: &str, decimals: u32) -> Result<Amount, CoreError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(CoreError::Validation("empty amount string".to_string()));
    }
    let (int_part, frac_part) = match trimmed.split_once('.') {
        Some((i, f)) => {
            if i.is_empty() || f.is_empty() {
                return Err(CoreError::Validation(
                    "malformed decimal amount: leading or trailing dot".to_string(),
                ));
            }
            (i, f)
        }
        None => (trimmed, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(CoreError::Validation("empty amount string".to_string()));
    }
    if !int_part.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return Err(CoreError::Validation(format!(
            "non-numeric characters in amount: {:?}",
            s
        )));
    }
    let scale = 10u128.checked_pow(decimals).ok_or(CoreError::Overflow)?;
    let mut units: u128 = 0;
    if !int_part.is_empty() {
        let whole: u128 = int_part.parse().map_err(|_| CoreError::Overflow)?;
        units = whole.checked_mul(scale).ok_or(CoreError::Overflow)?;
    }
    if !frac_part.is_empty() {
        if frac_part.len() > decimals as usize {
            return Err(CoreError::Validation(format!(
                "more than {} decimal places in amount: {:?}",
                decimals, s
            )));
        }
        let frac_val: u128 = frac_val_of(frac_part)?;
        let frac_scale = 10u128
            .checked_pow(decimals - frac_part.len() as u32)
            .ok_or(CoreError::Overflow)?;
        units = units
            .checked_add(
                frac_val
                    .checked_mul(frac_scale)
                    .ok_or(CoreError::Overflow)?,
            )
            .ok_or(CoreError::Overflow)?;
    }
    Ok(Amount(units))
}

fn frac_val_of(frac: &str) -> Result<u128, CoreError> {
    frac.parse()
        .map_err(|_| CoreError::Validation("bad fractional digits".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_units_scale_correctly() {
        let one = Amount::from_whole_units(1, 6).unwrap();
        assert_eq!(one.atomic(), 1_000_000);
        assert_eq!(Amount::from_whole_units(0, 6).unwrap(), Amount::ZERO);
    }

    #[test]
    fn decimal_parsing_round_trips() {
        let a: Amount = "1.5".parse().unwrap();
        assert_eq!(a.atomic(), 1_500_000);
        let b: Amount = "0.000001".parse().unwrap();
        assert_eq!(b.atomic(), 1);
        let c: Amount = "42".parse().unwrap();
        assert_eq!(c.atomic(), 42_000_000);
        let d: Amount = "1.500000".parse().unwrap();
        assert_eq!(a, d);
    }

    #[test]
    fn decimal_parsing_rejects_garbage() {
        for bad in [
            "",
            "  ",
            "-1",
            "1.",
            ".5",
            "1.0000001",
            "abc",
            "1 2",
            "1,5",
            "+1",
        ] {
            assert!(bad.parse::<Amount>().is_err(), "should reject {:?}", bad);
        }
    }

    #[test]
    fn formatting_is_lossless() {
        let a = Amount::from_atomic(1_500_000);
        assert_eq!(a.format(6), "1.500000");
        assert_eq!(Amount::ZERO.format(6), "0.000000");
        assert_eq!(Amount::from_atomic(7).format(6), "0.000007");
    }

    #[test]
    fn checked_arithmetic_reports_overflow() {
        let max = Amount::from_atomic(u128::MAX);
        assert!(max.checked_add(Amount::from_atomic(1)).is_err());
        assert!(Amount::ZERO.checked_sub(Amount::from_atomic(1)).is_err());
        let diff = Amount::from_atomic(2)
            .checked_sub(Amount::from_atomic(1))
            .unwrap();
        assert_eq!(diff, Amount::from_atomic(1));
    }

    #[test]
    fn order_follows_atomic_units() {
        assert!(Amount::from_atomic(1) < Amount::from_atomic(2));
        assert_eq!(Amount::default(), Amount::ZERO);
        assert!(Amount::ZERO.is_zero());
    }
}
