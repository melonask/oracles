//! X402 (HTTP 402 Payment Required) helpers.
//!
//! This module provides utilities for converting fiat amounts to crypto
//! asset amounts in their smallest unit, formatting rates, and checking
//! whether an asset has x402 payment metadata configured.

use crate::config::ResolvedAsset;
use crate::domain::RateAmount;
use crate::error::Result;
use rust_decimal::Decimal;

/// Convert a fiat amount to the smallest unit of a crypto asset.
///
/// Given a rate (e.g., 1 ETH = 3500.25 USD) and a fiat amount in USD,
/// returns the equivalent amount in the asset's smallest unit (wei for ETH,
/// lamports for SOL, base units for USDC, etc.).
///
/// Formula: `fiat_amount / rate * 10^decimals`
///
/// The result is floored to an integer number of base units, since
/// fractional base units do not exist on-chain.
pub fn convert_fiat_to_asset(
    fiat_amount: &RateAmount,
    rate: &RateAmount,
    decimals: u8,
) -> Result<RateAmount> {
    let base = Decimal::new(10_i64.pow(decimals as u32), 0);
    let amount = fiat_amount.decimal() / rate.decimal() * base;
    RateAmount::from_decimal(amount.floor())
}

/// Calculate the x402 price for an asset (smallest unit for a given USD
/// amount).
///
/// This is a convenience wrapper that takes an asset's resolved config
/// and the current rate to produce the x402-compatible price.
pub fn x402_price(
    asset: &ResolvedAsset,
    rate: &RateAmount,
    usd_amount: &RateAmount,
) -> Result<RateAmount> {
    convert_fiat_to_asset(usd_amount, rate, asset.decimals)
}

/// Format a rate with the appropriate number of decimal places.
///
/// Uses the given decimal-places count to control the precision of the
/// formatted string.
pub fn format_rate(rate: &RateAmount, decimals: u8) -> String {
    let value = rate.decimal();
    format!("{:.1$}", value, decimals as usize)
}

/// Check whether an asset has x402 payment metadata configured and enabled.
pub fn has_x402(asset: &ResolvedAsset) -> bool {
    asset.x402.as_ref().is_some_and(|x| x.enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ResolvedFeed, ResolvedX402Config};

    fn test_asset(decimals: u8, x402_enabled: bool) -> ResolvedAsset {
        ResolvedAsset {
            id: crate::domain::AssetId::new("test").expect("valid asset id"),
            enabled: true,
            chain_id: crate::domain::ChainId::new("test").expect("valid chain id"),
            caip2: "eip155:1".into(),
            symbol: "TEST".into(),
            name: Some("Test".into()),
            kind: "native".into(),
            contract: None,
            decimals,
            x402: x402_enabled.then(|| ResolvedX402Config {
                enabled: true,
                asset_address: "0x0000000000000000000000000000000000000001".into(),
                transfer_method: "permit2".into(),
            }),
            feeds: vec![ResolvedFeed {
                enabled: true,
                provider: crate::domain::ProviderId::new("static").expect("valid provider id"),
                priority: 100,
                params: std::collections::BTreeMap::new(),
            }],
            safety_enabled: false,
            safety_max_change_pct: None,
            safety_min_rate: None,
            safety_max_rate: None,
            safety_action: None,
        }
    }

    #[test]
    fn convert_eth_1_dollar_to_wei() {
        // ETH at $3500.25, 18 decimals
        let fiat = RateAmount::parse("1.00").expect("valid rate");
        let rate = RateAmount::parse("3500.25").expect("valid rate");
        let result = convert_fiat_to_asset(&fiat, &rate, 18).expect("valid conversion");
        // 1 / 3500.25 * 10^18 ≈ 285,693,879,008,642 wei (floored)
        assert!(result.decimal() > Decimal::new(285_000_000_000_000_i64, 0));
        assert!(result.decimal() < Decimal::new(286_000_000_000_000_i64, 0));
    }

    #[test]
    fn convert_usdc_1_dollar_to_base_units() {
        // USDC at $1.00, 6 decimals
        let fiat = RateAmount::parse("1.00").expect("valid rate");
        let rate = RateAmount::parse("1.00").expect("valid rate");
        let result = convert_fiat_to_asset(&fiat, &rate, 6).expect("valid conversion");
        assert_eq!(result.decimal(), Decimal::new(1_000_000, 0));
    }

    #[test]
    fn convert_sol_100_dollars_to_lamports() {
        // SOL at $150.00, 9 decimals
        let fiat = RateAmount::parse("100.00").expect("valid rate");
        let rate = RateAmount::parse("150.00").expect("valid rate");
        let result = convert_fiat_to_asset(&fiat, &rate, 9).expect("valid conversion");
        // 100 / 150 * 10^9 = 666,666,666.66... floor => 666,666,666
        assert_eq!(result.decimal(), Decimal::new(666_666_666_i64, 0));
    }

    #[test]
    fn convert_small_amount_rounds_down_to_zero() {
        let fiat = RateAmount::parse("0.000001").expect("valid rate"); // tiny
        let rate = RateAmount::parse("50000.00").expect("valid rate"); // expensive asset
        let result = convert_fiat_to_asset(&fiat, &rate, 6);
        // 0.000001 / 50000 * 10^6 = 0.00002 → floor = 0
        // RateAmount::from_decimal(0) returns Err (zero not allowed)
        assert!(result.is_err());
    }

    #[test]
    fn x402_price_uses_asset_decimals() {
        let asset = test_asset(6, true);
        let rate = RateAmount::parse("1.00").expect("valid rate");
        let usd = RateAmount::parse("5.00").expect("valid rate");
        let price = x402_price(&asset, &rate, &usd).expect("valid price");
        assert_eq!(price.decimal(), Decimal::new(5_000_000, 0));
    }

    #[test]
    fn format_rate_basic() {
        let rate = RateAmount::parse("3500.25").expect("valid rate");
        // 2 decimal places
        assert_eq!(format_rate(&rate, 2), "3500.25");
        // 6 decimal places
        assert_eq!(format_rate(&rate, 6), "3500.250000");
        // 0 decimal places
        assert_eq!(format_rate(&rate, 0), "3500");
    }

    #[test]
    fn format_rate_with_truncation() {
        // 1.2345 with 4 decimal places preserves all digits
        let rate = RateAmount::parse("1.2345").expect("valid rate");
        assert_eq!(format_rate(&rate, 4), "1.2345");
        // 1.2300 with 2 decimal places
        let rate = RateAmount::parse("1.23").expect("valid rate");
        assert_eq!(format_rate(&rate, 2), "1.23");
    }

    #[test]
    fn has_x402_when_configured() {
        let asset = test_asset(18, true);
        assert!(has_x402(&asset));
    }

    #[test]
    fn has_x402_false_when_disabled_in_config() {
        let mut asset = test_asset(18, true);
        asset.x402.as_mut().expect("x402 present").enabled = false;
        assert!(!has_x402(&asset));
    }

    #[test]
    fn has_x402_false_when_missing() {
        let asset = test_asset(18, false);
        assert!(!has_x402(&asset));
    }
}
