#![allow(missing_docs)]
#![cfg(feature = "sqlite")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use oracles::config::{
    BootstrapAction, CompareAgainst, EventMode, ProviderKind, ResolvedAsset, ResolvedConfig,
    ResolvedConsensusConfig, ResolvedEventColumns, ResolvedEventsConfig, ResolvedFeed,
    ResolvedHttpConfig, ResolvedOraclesConfig, ResolvedOutboxColumns, ResolvedOutboxConfig,
    ResolvedProvider, ResolvedRateColumns, ResolvedRateTableConfig, ResolvedSafetyConfig,
    ResolvedStoreConfig, SelectionMode, StoreDriver, WriteMode,
};
use oracles::domain::{AssetId, ChainId, EventAction, ProviderId, Quote, RateAmount};
use oracles::engine::Oracle;
use oracles::store::sqlite::SqliteRateStore;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::sync::Arc;
use time::Duration;

#[test]
fn static_provider_through_safety_to_store() -> oracles::Result<()> {
    let config = test_config("1000")?;
    let store = SqliteRateStore::open(&config)?;
    let providers: Vec<Arc<dyn oracles::provider::Provider>> = vec![Arc::new(
        oracles::provider::static_provider::StaticProvider::new(ProviderId::new("static")?),
    )];
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once()?;
    assert_eq!(summary.attempted, 1);
    assert_eq!(summary.succeeded, 1);
    assert_eq!(summary.failed, 0);
    Ok(())
}

fn test_config(static_rate: &str) -> oracles::Result<ResolvedConfig> {
    let store_id = "oracles".to_owned();
    let asset_id = AssetId::new("eth")?;
    let chain_id = ChainId::new("eth")?;
    let provider_id = ProviderId::new("static")?;
    let quote = Quote::new("USD")?;
    let mut stores = BTreeMap::new();
    stores.insert(
        store_id.clone(),
        ResolvedStoreConfig {
            driver: StoreDriver::Sqlite,
            url: "sqlite::memory:".to_owned(),
            migrate: true,
            connect_timeout_secs: 10,
            max_connections: 1,
        },
    );
    let mut providers_map = BTreeMap::new();
    providers_map.insert(
        provider_id.clone(),
        ResolvedProvider {
            id: provider_id.clone(),
            kind: ProviderKind::Static,
            method: None,
            url_template: None,
            auth: None,
            rate_path: None,
            source_updated_at_path: None,
            source_updated_at_format: None,
        },
    );
    let mut params = BTreeMap::new();
    params.insert("rate".to_owned(), static_rate.to_owned());
    Ok(ResolvedConfig {
        version: 1,
        log: oracles::config::ResolvedLogConfig {
            level: "info".to_owned(),
            format: "json".to_owned(),
        },
        stores,
        http: ResolvedHttpConfig {
            user_agent: "oracles-test/0.1".to_owned(),
            request_timeout_secs: 15,
            max_retries: 0,
            retry_backoff_ms: 0,
        },
        chains: BTreeMap::new(),
        assets: vec![ResolvedAsset {
            id: asset_id,
            enabled: true,
            chain_id,
            caip2: "eip155:1".to_owned(),
            symbol: "ETH".to_owned(),
            name: Some("Ether".to_owned()),
            kind: "native".to_owned(),
            contract: None,
            decimals: 18,
            x402: None,
            feeds: vec![ResolvedFeed {
                enabled: true,
                provider: provider_id,
                priority: 100,
                params,
            }],
            safety_enabled: true,
            safety_max_change_pct: Some(Decimal::new(50, 0)),
            safety_min_rate: Some(RateAmount::parse("1")?),
            safety_max_rate: Some(RateAmount::parse("100000")?),
            safety_action: Some(EventAction::Quarantine),
        }],
        oracles: ResolvedOraclesConfig {
            store: store_id.clone(),
            quote,
            refresh_secs: 180,
            stale_after_secs: 300,
            max_source_age_secs: Some(900),
            max_concurrent_requests: 8,
            fail_fast: false,
            selection: SelectionMode::Priority,
            table: ResolvedRateTableConfig {
                name: "oracle_rates".to_owned(),
                write_mode: WriteMode::Upsert,
                columns: ResolvedRateColumns::defaults(),
            },
        },
        safety: ResolvedSafetyConfig {
            enabled: true,
            compare_against: CompareAgainst::LastAccepted,
            default_action: EventAction::Quarantine,
            max_change_pct: Decimal::new(50, 0),
            min_rate: Some(RateAmount::parse("0.00000001")?),
            max_rate: None,
            max_source_age: Some(Duration::seconds(900)),
            stale_after: Duration::seconds(300),
            alert_cooldown_secs: 3600,
            record_anomalies: true,
            bootstrap_action: BootstrapAction::Accept,
            consensus: ResolvedConsensusConfig {
                min_successful_feeds: 1,
                max_provider_spread_pct: Decimal::new(5, 0),
                action: EventAction::Quarantine,
            },
        },
        events: ResolvedEventsConfig {
            enabled: true,
            mode: EventMode::Simple,
            store: store_id.clone(),
            record: true,
            table: "oracle_events".to_owned(),
            sink_fail_fast: false,
            columns: ResolvedEventColumns::defaults(),
            routes: Vec::new(),
            sinks: BTreeMap::new(),
        },
        outbox: ResolvedOutboxConfig {
            enabled: false,
            store: store_id.clone(),
            table: "oracle_outbox".to_owned(),
            dispatch_interval_secs: 10,
            max_retries: 5,
            retry_backoff_secs: 30,
            request_timeout_secs: 10,
            columns: ResolvedOutboxColumns::defaults(),
        },
        providers: providers_map,
    })
}
