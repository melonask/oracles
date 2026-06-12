#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use oracles::config::raw::{
    RawAssetConfig, RawConfig, RawFeedConfig, RawOraclesConfig, RawProviderConfig, RawStoreConfig,
    RawTransportsConfig,
};
use oracles::config::validate::resolve_config;
use std::collections::BTreeMap;

#[test]
fn missing_chain_reference_fails_validation() {
    let mut stores = BTreeMap::new();
    stores.insert(
        "oracles".to_owned(),
        RawStoreConfig {
            driver: "sqlite".to_owned(),
            url: "sqlite::memory:".to_owned(),
            migrate: None,
            connect_timeout_secs: None,
            max_connections: None,
        },
    );

    let mut providers = BTreeMap::new();
    providers.insert(
        "static".to_owned(),
        RawProviderConfig {
            kind: "static".to_owned(),
            method: None,
            url_template: None,
            transport: None,
            auth: None,
            paths: None,
            formats: None,
        },
    );

    let mut assets = BTreeMap::new();
    let mut params = BTreeMap::new();
    params.insert("rate".to_owned(), "1000".to_owned());

    assets.insert(
        "eth".to_owned(),
        RawAssetConfig {
            enabled: None,
            chain: "missing_chain".to_owned(),
            symbol: "ETH".to_owned(),
            name: None,
            kind: "native".to_owned(),
            contract: None,
            decimals: 18,
            x402: None,
            feeds: Some(vec![RawFeedConfig {
                enabled: None,
                provider: "static".to_owned(),
                priority: 100,
                params: Some(params),
            }]),
            safety: None,
        },
    );

    let raw = RawConfig {
        version: 1,
        log: None,
        stores,
        http: None,
        chains: BTreeMap::new(),
        assets,
        oracles: RawOraclesConfig {
            store: "oracles".to_owned(),
            quote: "USD".to_owned(),
            refresh_secs: 180,
            stale_after_secs: 300,
            max_source_age_secs: None,
            max_concurrent_requests: None,
            fail_fast: None,
            selection: None,
            table: None,
            safety: None,
            events: None,
            outbox: None,
            providers,
            asset_ids: None,
            assets: None,
            enabled: None,
        },
    };

    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(result.is_err());

    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("unknown chain"),
        "expected unknown chain error, got: {err}"
    );
}
