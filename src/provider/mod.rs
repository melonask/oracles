//! Provider abstraction for fetching cryptocurrency rates.

use crate::config::{ResolvedAsset, ResolvedFeed, ResolvedProvider};
use crate::domain::{CandidateRate, ProviderId, Quote};
use crate::error::Result;
use std::sync::Arc;
use time::OffsetDateTime;

/// Static (config-defined) rate provider.
pub mod static_provider;

#[cfg(feature = "http-json")]
/// HTTP JSON API rate provider.
pub mod http_json;

/// JSON path traversal utilities.
pub mod json_path;
/// String template rendering with `{placeholder}` substitution.
pub mod template;

/// A rate provider that fetches a [`CandidateRate`] for a given asset and feed.
pub trait Provider: Send + Sync {
    /// Return this provider's unique identifier.
    fn id(&self) -> &ProviderId;

    /// Fetch a candidate rate from this provider.
    ///
    /// The `asset`, `feed`, `provider` config, and `ctx` provide all the
    /// information needed to construct the request and parse the response.
    fn fetch(
        &self,
        asset: &ResolvedAsset,
        feed: &ResolvedFeed,
        provider: &ResolvedProvider,
        ctx: &ProviderContext,
    ) -> Result<CandidateRate>;
}

/// Context passed to providers during a fetch cycle.
///
/// All data is owned so the context can be shared across threads during
/// concurrent provider fetching.
pub struct ProviderContext {
    /// The quote currency for this fetch.
    pub quote: Quote,
    /// The timestamp of this observation.
    pub observed_at: OffsetDateTime,
    /// The `User-Agent` header to use.
    pub user_agent: String,
    /// Request timeout in seconds.
    pub request_timeout_secs: u64,
    /// Maximum retry attempts.
    pub max_retries: u32,
    /// Retry backoff delay in milliseconds.
    pub retry_backoff_ms: u64,
}

/// Build provider instances from a resolved configuration.
///
/// Each configured provider is instantiated with the appropriate kind
/// (static, HTTP JSON, etc.) and returned as an [`Arc`]-wrapped trait object
/// so they can be shared across concurrent fetch threads.
pub fn build_providers(config: &crate::config::ResolvedConfig) -> Result<Vec<Arc<dyn Provider>>> {
    let mut providers: Vec<Arc<dyn Provider>> = Vec::new();

    for (id, pconfig) in &config.providers {
        match pconfig.kind {
            crate::config::ProviderKind::Static => {
                providers.push(Arc::new(static_provider::StaticProvider::new(id.clone())));
            }
            #[cfg(feature = "http-json")]
            crate::config::ProviderKind::HttpJson => {
                providers.push(Arc::new(http_json::HttpJsonProvider::new(id.clone())));
            }
            #[cfg(not(feature = "http-json"))]
            crate::config::ProviderKind::HttpJson => {
                return Err(crate::error::Error::Config(
                    "http-json feature is not enabled".to_owned(),
                ));
            }
        }
    }

    Ok(providers)
}
