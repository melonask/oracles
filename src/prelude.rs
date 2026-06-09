//! Convenience re-exports of the most commonly used types.
//!
//! Use `use oracles::prelude::*;` to import the core domain types,
//! error types, and key traits in one go.

pub use crate::domain::{
    AssetId, CandidateRate, ChainId, OracleEvent, ProviderId, Quote, RateAmount, RateRecord,
};
pub use crate::engine::Oracle;
pub use crate::error::{Error, Result};
pub use crate::provider::Provider;
pub use crate::store::RateStore;
pub use crate::x402::{convert_fiat_to_asset, format_rate, has_x402, x402_price};
