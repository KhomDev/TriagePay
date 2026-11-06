//! SKU price book: fixed prices for metered tool calls.
//!
//! Prices are integer amounts set at configuration time. The default
//! book prices one deterministic finding scan (sku scan.v1) at exactly
//! 1 USDC, and one two-stage triage (sku triage.v2: deterministic gates
//! plus the advisory model judge) at a 1 USDC PLACEHOLDER until the
//! benchmark evaluation measures the cost per call.

use crate::error::CoreError;
use crate::money::Amount;
use std::collections::HashMap;

/// Canonical SKU of one finding scan.
pub const SCAN_SKU: &str = "scan.v1";

/// Canonical SKU of one two-stage triage (Stage A gates plus Stage B model).
pub const TRIAGE_V2_SKU: &str = "triage.v2";

/// Placeholder price of `triage.v2` in whole USDC. It is NOT derived from a
/// measured cost: the benchmark evaluation (prompt 05) replaces it with the
/// measured cost per call plus margin.
pub const TRIAGE_V2_PLACEHOLDER_USDC: u64 = 1;

/// Fixed prices for metered tool SKUs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceBook {
    prices: HashMap<String, Amount>,
}

impl PriceBook {
    /// An empty book.
    pub fn new() -> Self {
        PriceBook {
            prices: HashMap::new(),
        }
    }

    /// The default book: one scan at exactly 1 USDC, and one triage.v2 call
    /// at the placeholder price (see [`TRIAGE_V2_PLACEHOLDER_USDC`]).
    pub fn default_book() -> Self {
        let mut book = PriceBook::new();
        book.set_price(
            SCAN_SKU,
            Amount::from_whole_units(1, crate::money::SETTLEMENT_DECIMALS)
                .expect("1 USDC at 6 decimals is representable"),
        )
        .expect("scan.v1 price is positive by construction");
        book.set_price(
            TRIAGE_V2_SKU,
            Amount::from_whole_units(
                TRIAGE_V2_PLACEHOLDER_USDC,
                crate::money::SETTLEMENT_DECIMALS,
            )
            .expect("the placeholder at 6 decimals is representable"),
        )
        .expect("triage.v2 placeholder price is positive by construction");
        book
    }

    /// Register or update the price of a SKU. Zero prices are rejected:
    /// free work is a configuration mistake, not a feature.
    pub fn set_price(&mut self, sku: &str, price: Amount) -> Result<(), CoreError> {
        if sku.trim().is_empty() {
            return Err(CoreError::Validation("sku must be non-empty".to_string()));
        }
        if price.is_zero() {
            return Err(CoreError::Validation(
                "sku price must be positive".to_string(),
            ));
        }
        self.prices.insert(sku.to_string(), price);
        Ok(())
    }

    /// The price of a SKU, if configured.
    pub fn price(&self, sku: &str) -> Option<Amount> {
        self.prices.get(sku).copied()
    }

    /// Number of priced SKUs.
    pub fn len(&self) -> usize {
        self.prices.len()
    }

    /// True when no SKU is priced.
    pub fn is_empty(&self) -> bool {
        self.prices.is_empty()
    }
}

impl Default for PriceBook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_book_prices_one_scan_at_one_usdc() {
        let book = PriceBook::default_book();
        let price = book.price(SCAN_SKU).expect("scan.v1 is priced");
        assert_eq!(price.atomic(), 1_000_000);
        assert_eq!(price.format(crate::money::SETTLEMENT_DECIMALS), "1.000000");
    }

    #[test]
    fn zero_and_blank_prices_are_rejected() {
        let mut book = PriceBook::new();
        assert!(book.set_price("scan.v1", Amount::ZERO).is_err());
        assert!(book.set_price("   ", Amount::from_atomic(1)).is_err());
        assert!(book.is_empty());
    }

    #[test]
    fn unknown_sku_has_no_price() {
        let book = PriceBook::default_book();
        assert!(book.price("scan.v9").is_none());
        assert!(book.price("triage.v1").is_none());
        // scan.v1 and triage.v2.
        assert_eq!(book.len(), 2);
    }

    #[test]
    fn default_book_prices_triage_v2_at_the_placeholder() {
        let book = PriceBook::default_book();
        let price = book.price(TRIAGE_V2_SKU).expect("triage.v2 is priced");
        assert_eq!(TRIAGE_V2_SKU, "triage.v2");
        assert_eq!(TRIAGE_V2_PLACEHOLDER_USDC, 1);
        assert_eq!(price.atomic(), 1_000_000);
        assert_eq!(price.format(crate::money::SETTLEMENT_DECIMALS), "1.000000");
    }

    #[test]
    fn triage_v2_price_is_independent_of_scan_v1() {
        let mut book = PriceBook::default_book();
        book.set_price(TRIAGE_V2_SKU, Amount::from_whole_units(3, 6).unwrap())
            .unwrap();
        assert_eq!(book.price(TRIAGE_V2_SKU).unwrap().atomic(), 3_000_000);
        assert_eq!(book.price(SCAN_SKU).unwrap().atomic(), 1_000_000);
    }

    #[test]
    fn prices_can_be_updated() {
        let mut book = PriceBook::default_book();
        book.set_price(SCAN_SKU, Amount::from_whole_units(2, 6).unwrap())
            .unwrap();
        assert_eq!(book.price(SCAN_SKU).unwrap().atomic(), 2_000_000);
    }
}
