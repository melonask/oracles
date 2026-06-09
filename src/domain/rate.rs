use crate::domain::{AssetId, ChainId, ProviderId, Quote, RateAmount};
use time::OffsetDateTime;

/// A rate candidate produced by a provider before safety evaluation.
///
/// This is the raw rate fetched from a provider (static, HTTP JSON, etc.).
/// It has not yet been validated by the safety engine.
#[derive(Clone, Debug)]
pub struct CandidateRate {
    /// The asset identifier (e.g., `"eth"`).
    pub asset_id: AssetId,
    /// The chain identifier (e.g., `"eth"` for Ethereum mainnet).
    pub chain_id: ChainId,
    /// The CAIP-2 chain identifier (e.g., `"eip155:1"`).
    pub caip2: String,
    /// The asset symbol (e.g., `"ETH"`).
    pub symbol: String,
    /// The quote currency (e.g., `"USD"`).
    pub quote: Quote,
    /// The provider that produced this rate.
    pub provider: ProviderId,
    /// The candidate rate value.
    pub rate: RateAmount,
    /// When the source data was last updated (if known).
    pub source_updated_at: Option<OffsetDateTime>,
    /// When this candidate was observed/fetched.
    pub observed_at: OffsetDateTime,
}

/// A rate that has been accepted by the safety engine and persisted.
///
/// Unlike [`CandidateRate`], this record includes an `expires_at` timestamp
/// indicating when the rate should be considered stale.
#[derive(Clone, Debug)]
pub struct RateRecord {
    /// The asset identifier.
    pub asset_id: AssetId,
    /// The chain identifier.
    pub chain_id: ChainId,
    /// The CAIP-2 chain identifier.
    pub caip2: String,
    /// The asset symbol.
    pub symbol: String,
    /// The quote currency.
    pub quote: Quote,
    /// The provider that produced this rate.
    pub provider: ProviderId,
    /// The accepted rate value.
    pub rate: RateAmount,
    /// When the source data was last updated (if known).
    pub source_updated_at: Option<OffsetDateTime>,
    /// When this rate was observed/fetched.
    pub observed_at: OffsetDateTime,
    /// When this rate expires and should be considered stale.
    pub expires_at: OffsetDateTime,
}
