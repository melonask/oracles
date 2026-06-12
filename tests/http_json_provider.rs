#![allow(missing_docs)]
#![cfg(feature = "http-json")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use oracles::config::{ProviderKind, ResolvedAsset, ResolvedFeed, ResolvedProvider};
use oracles::domain::{AssetId, ChainId, ProviderId, Quote};
use oracles::provider::ProviderContext;
use oracles::provider::http_json::parse_http_json_candidate;
use std::collections::BTreeMap;
use time::OffsetDateTime;

fn eth_asset() -> ResolvedAsset {
    ResolvedAsset {
        id: AssetId::new("eth").unwrap(),
        enabled: true,
        chain_id: ChainId::new("eth").unwrap(),
        caip2: "eip155:1".to_owned(),
        symbol: "ETH".to_owned(),
        name: Some("Ether".to_owned()),
        kind: "native".to_owned(),
        contract: None,
        decimals: 18,
        x402: None,
        feeds: vec![ResolvedFeed {
            enabled: true,
            provider: ProviderId::new("diadata").unwrap(),
            priority: 100,
            params: BTreeMap::new(),
        }],
        safety_enabled: true,
        safety_max_change_pct: None,
        safety_min_rate: None,
        safety_max_rate: None,
        safety_action: None,
    }
}

fn context() -> ProviderContext {
    ProviderContext {
        quote: Quote::new("USD").unwrap(),
        observed_at: OffsetDateTime::now_utc(),
        user_agent: "oracles-test/0.1".to_owned(),
        request_timeout_secs: 15,
        max_retries: 0,
        retry_backoff_ms: 0,
    }
}

fn diadata_provider() -> ResolvedProvider {
    ResolvedProvider {
        id: ProviderId::new("diadata").unwrap(),
        kind: ProviderKind::HttpJson,
        method: Some("GET".to_owned()),
        url_template: Some("https://api.diadata.org/v1/assetQuotation/Ethereum/0x0000000000000000000000000000000000000000".to_owned()),
        transport: None,
        auth: None,
        rate_path: Some("Price".to_owned()),
        source_updated_at_path: Some("Time".to_owned()),
        source_updated_at_format: Some("rfc3339".to_owned()),
    }
}

fn coingecko_provider() -> ResolvedProvider {
    ResolvedProvider {
        id: ProviderId::new("coingecko").unwrap(),
        kind: ProviderKind::HttpJson,
        method: Some("GET".to_owned()),
        url_template: Some(
            "https://api.coingecko.com/api/v3/simple/price?ids=ethereum&vs_currencies=usd"
                .to_owned(),
        ),
        transport: None,
        auth: None,
        rate_path: Some("ethereum.usd".to_owned()),
        source_updated_at_path: Some("ethereum.last_updated_at".to_owned()),
        source_updated_at_format: Some("unix".to_owned()),
    }
}

#[test]
fn parse_diadata_response() {
    let asset = eth_asset();
    let feed = &asset.feeds[0];
    let provider = diadata_provider();
    let ctx = context();

    let body = include_str!("fixtures/diadata_eth.json");
    let candidate = parse_http_json_candidate(&asset, feed, &provider, &ctx, body).unwrap();

    assert_eq!(candidate.rate.to_string(), "3500.25");
    assert_eq!(candidate.asset_id.as_str(), "eth");
    assert_eq!(candidate.quote.as_str(), "USD");
    assert!(candidate.source_updated_at.is_some());
}

#[test]
fn parse_coingecko_response() {
    let asset = eth_asset();
    let feed = &asset.feeds[0];
    let provider = coingecko_provider();
    let ctx = context();

    let body = include_str!("fixtures/coingecko_eth.json");
    let candidate = parse_http_json_candidate(&asset, feed, &provider, &ctx, body).unwrap();

    assert_eq!(candidate.rate.to_string(), "3501.75");
    assert_eq!(candidate.asset_id.as_str(), "eth");
    assert_eq!(candidate.quote.as_str(), "USD");
    assert!(candidate.source_updated_at.is_some());
}
