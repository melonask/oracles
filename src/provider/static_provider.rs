use crate::config::{ResolvedAsset, ResolvedFeed, ResolvedProvider};
use crate::domain::{CandidateRate, ProviderId, RateAmount};
use crate::error::{Error, Result};
use crate::provider::{Provider, ProviderContext};

/// A provider that returns a fixed rate from configuration parameters.
///
/// The rate is read from `feed.params.get("rate")` and must be a valid
/// decimal string (e.g., `"3500.25"`).
pub struct StaticProvider {
    id: ProviderId,
}

impl StaticProvider {
    /// Create a new [`StaticProvider`] with the given ID.
    pub fn new(id: ProviderId) -> Self {
        Self { id }
    }
}

impl Provider for StaticProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn fetch(
        &self,
        asset: &ResolvedAsset,
        feed: &ResolvedFeed,
        _provider: &ResolvedProvider,
        ctx: &ProviderContext,
    ) -> Result<CandidateRate> {
        let rate = feed
            .params
            .get("rate")
            .ok_or_else(|| Error::Provider("static provider requires params.rate".to_owned()))?;

        Ok(CandidateRate {
            asset_id: asset.id.clone(),
            chain_id: asset.chain_id.clone(),
            caip2: asset.caip2.clone(),
            symbol: asset.symbol.clone(),
            quote: ctx.quote.clone(),
            provider: self.id.clone(),
            rate: RateAmount::parse(rate)?,
            source_updated_at: None,
            observed_at: ctx.observed_at,
        })
    }
}
