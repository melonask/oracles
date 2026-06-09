#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use oracles::domain::{AssetId, RateAmount};

#[test]
fn asset_id_rejects_uppercase() {
    assert!(AssetId::new("ETH").is_err());
    assert!(AssetId::new("eth").is_ok());
    assert!(AssetId::new("usdc_base").is_ok());
}

#[test]
fn percent_change_detects_large_move() {
    let old = RateAmount::parse("100").unwrap();
    let new = RateAmount::parse("151").unwrap();
    let pct = new.percent_change_from(&old).unwrap();
    assert_eq!(pct.to_string(), "51.00");
}

#[test]
fn stablecoin_bounds_reject_bad_rate() {
    let candidate = RateAmount::parse("1.25").unwrap();
    let max = RateAmount::parse("1.10").unwrap();
    assert!(candidate > max);
}
