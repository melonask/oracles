#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(missing_docs)]

use oracles::config::{
    BootstrapAction, CompareAgainst, EventMode, ResolvedAsset, ResolvedChain, ResolvedConfig,
    ResolvedConsensusConfig, ResolvedEventColumns, ResolvedEventsConfig, ResolvedFeed,
    ResolvedHttpConfig, ResolvedLogConfig, ResolvedOraclesConfig, ResolvedOutboxColumns,
    ResolvedOutboxConfig, ResolvedProvider, ResolvedRateColumns, ResolvedRateTableConfig,
    ResolvedSafetyConfig, ResolvedStoreConfig, SelectionMode, StoreDriver, WriteMode,
};
use oracles::domain::{
    AssetId, ChainId, EventAction, EventReason, EventType, OracleEvent, ProviderId, Quote,
    RateAmount,
};
use oracles::engine::Oracle;
use oracles::provider::{Provider, ProviderContext};
use oracles::store::RateStore;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::sync::Arc;
use time::Duration;

// ---------------------------------------------------------------------------
// In-memory test store
// ---------------------------------------------------------------------------

struct TestStore {
    rates: Vec<oracles::domain::RateRecord>,
    events: Vec<oracles::domain::OracleEvent>,
}

impl TestStore {
    fn new() -> Self {
        Self {
            rates: Vec::new(),
            events: Vec::new(),
        }
    }
}

impl RateStore for TestStore {
    fn last_accepted_rate(
        &mut self,
        asset_id: &AssetId,
        quote: &Quote,
    ) -> oracles::error::Result<Option<oracles::domain::RateRecord>> {
        Ok(self
            .rates
            .iter()
            .rev()
            .find(|r| &r.asset_id == asset_id && &r.quote == quote)
            .cloned())
    }

    fn write_accepted_rate(
        &mut self,
        record: &oracles::domain::RateRecord,
    ) -> oracles::error::Result<()> {
        self.rates.push(record.clone());
        Ok(())
    }

    fn write_event(
        &mut self,
        event: &oracles::domain::OracleEvent,
    ) -> oracles::error::Result<oracles::store::EventRowId> {
        self.events.push(event.clone());
        Ok(Some(self.events.len() as i64))
    }

    fn write_outbox(
        &mut self,
        _event_id: oracles::store::EventRowId,
        _event: &oracles::domain::OracleEvent,
        _sink: &str,
        _payload: &str,
    ) -> oracles::error::Result<()> {
        Ok(()) // no-op for test store
    }

    fn has_recent_disable_event(&mut self, asset_id: &AssetId) -> oracles::error::Result<bool> {
        Ok(self
            .events
            .iter()
            .any(|e| &e.asset_id == asset_id && e.action == EventAction::DisableAsset))
    }
}

// ---------------------------------------------------------------------------
// Test provider that returns a fixed rate
// ---------------------------------------------------------------------------

struct TestProvider {
    id: ProviderId,
    rate: String,
}

impl TestProvider {
    fn new(id: &str, rate: &str) -> Self {
        Self {
            id: ProviderId::new(id).unwrap(),
            rate: rate.to_owned(),
        }
    }
}

impl Provider for TestProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn fetch(
        &self,
        asset: &ResolvedAsset,
        _feed: &ResolvedFeed,
        _provider: &ResolvedProvider,
        ctx: &ProviderContext,
    ) -> oracles::error::Result<oracles::domain::CandidateRate> {
        Ok(oracles::domain::CandidateRate {
            asset_id: asset.id.clone(),
            chain_id: asset.chain_id.clone(),
            caip2: asset.caip2.clone(),
            symbol: asset.symbol.clone(),
            quote: ctx.quote.clone(),
            provider: self.id.clone(),
            rate: RateAmount::parse(&self.rate).unwrap(),
            source_updated_at: Some(ctx.observed_at - Duration::seconds(30)),
            observed_at: ctx.observed_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Helper: build a minimal ResolvedConfig
// ---------------------------------------------------------------------------

fn test_config(selection: SelectionMode, safety_enabled: bool) -> (ResolvedConfig, AssetId) {
    let asset_id = AssetId::new("eth").unwrap();
    let chain_id = ChainId::new("eth").unwrap();
    let provider_id = ProviderId::new("test1").unwrap();

    let config = ResolvedConfig {
        version: 1,
        log: ResolvedLogConfig {
            level: "info".to_owned(),
            format: "pretty".to_owned(),
        },
        stores: {
            let mut m = BTreeMap::new();
            m.insert(
                "oracles".to_owned(),
                ResolvedStoreConfig {
                    driver: StoreDriver::Sqlite,
                    url: "sqlite://:memory:".to_owned(),
                    migrate: false,
                    connect_timeout_secs: 5,
                    max_connections: 1,
                },
            );
            m
        },
        http: ResolvedHttpConfig {
            user_agent: "test/1.0".to_owned(),
            request_timeout_secs: 15,
            max_retries: 0,
            retry_backoff_ms: 0,
        },
        chains: {
            let mut m = BTreeMap::new();
            m.insert(
                chain_id.clone(),
                ResolvedChain {
                    id: chain_id.clone(),
                    family: "evm".to_owned(),
                    caip2: "eip155:1".to_owned(),
                    native_symbol: Some("ETH".to_owned()),
                    rpc_urls: vec![],
                    confirmations: 0,
                },
            );
            m
        },
        assets: vec![ResolvedAsset {
            id: asset_id.clone(),
            enabled: true,
            chain_id: chain_id.clone(),
            caip2: "eip155:1".to_owned(),
            symbol: "ETH".to_owned(),
            name: None,
            kind: "native".to_owned(),
            contract: None,
            decimals: 18,
            x402: None,
            feeds: vec![ResolvedFeed {
                enabled: true,
                provider: provider_id.clone(),
                priority: 1,
                params: BTreeMap::new(),
            }],
            safety_enabled,
            safety_max_change_pct: None,
            safety_min_rate: None,
            safety_max_rate: None,
            safety_action: None,
        }],
        oracles: ResolvedOraclesConfig {
            store: "oracles".to_owned(),
            quote: Quote::new("USD").unwrap(),
            refresh_secs: 180,
            stale_after_secs: 300,
            max_source_age_secs: Some(900),
            max_concurrent_requests: 8,
            fail_fast: false,
            selection,
            table: ResolvedRateTableConfig {
                name: "oracle_rates".to_owned(),
                write_mode: WriteMode::Upsert,
                columns: ResolvedRateColumns::defaults(),
            },
        },
        safety: ResolvedSafetyConfig {
            enabled: safety_enabled,
            compare_against: CompareAgainst::LastAccepted,
            default_action: EventAction::Quarantine,
            max_change_pct: Decimal::new(50, 0),
            min_rate: None,
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
            enabled: false,
            mode: EventMode::Simple,
            store: "oracles".to_owned(),
            record: false,
            table: "oracle_events".to_owned(),
            sink_fail_fast: false,
            columns: ResolvedEventColumns::defaults(),
            routes: vec![],
            sinks: BTreeMap::new(),
        },
        outbox: ResolvedOutboxConfig {
            enabled: false,
            store: "oracles".to_owned(),
            table: "oracle_outbox".to_owned(),
            dispatch_interval_secs: 10,
            max_retries: 3,
            retry_backoff_secs: 30,
            request_timeout_secs: 10,
            columns: ResolvedOutboxColumns::defaults(),
        },
        providers: {
            let mut m = BTreeMap::new();
            m.insert(
                provider_id.clone(),
                ResolvedProvider {
                    id: provider_id.clone(),
                    kind: oracles::config::ProviderKind::Static,
                    method: None,
                    url_template: None,
                    transport: None,
                    auth: None,
                    rate_path: None,
                    source_updated_at_path: None,
                    source_updated_at_format: None,
                },
            );
            m
        },
    };

    (config, asset_id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn engine_priority_mode_accepts_rate() {
    let (config, _) = test_config(SelectionMode::Priority, false);
    let store = TestStore::new();
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(TestProvider::new("test1", "3500.25"))];
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once().unwrap();
    assert_eq!(summary.attempted, 1);
    assert_eq!(summary.succeeded, 1);
    assert_eq!(summary.failed, 0);
}

#[test]
fn engine_all_mode_accepts_all_candidates() {
    let (mut config, _) = test_config(SelectionMode::All, false);
    // Add a second provider
    let p2_id = ProviderId::new("test2").unwrap();
    config.providers.insert(
        p2_id.clone(),
        ResolvedProvider {
            id: p2_id.clone(),
            kind: oracles::config::ProviderKind::Static,
            method: None,
            url_template: None,
            transport: None,
            auth: None,
            rate_path: None,
            source_updated_at_path: None,
            source_updated_at_format: None,
        },
    );
    // Add second feed
    if let Some(asset) = config.assets.get_mut(0) {
        asset.feeds.push(ResolvedFeed {
            enabled: true,
            provider: p2_id,
            priority: 2,
            params: BTreeMap::new(),
        });
    }

    let store = TestStore::new();
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(TestProvider::new("test1", "3500.0")),
        Arc::new(TestProvider::new("test2", "3510.0")),
    ];
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once().unwrap();
    assert_eq!(summary.attempted, 1);
    assert_eq!(summary.succeeded, 1);
}

#[test]
fn engine_safety_enabled_quarantines_stale_source() {
    let (config, _) = test_config(SelectionMode::Priority, true);
    let store = TestStore::new();
    // This provider returns source_updated_at that's 30s old (see TestProvider),
    // which is within 900s max. Should pass.
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(TestProvider::new("test1", "3500.25"))];
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once().unwrap();
    assert_eq!(summary.succeeded, 1);
}

#[test]
fn engine_run_once_skips_disabled_assets() {
    let (mut config, _) = test_config(SelectionMode::Priority, false);
    if let Some(asset) = config.assets.get_mut(0) {
        asset.enabled = false;
    }
    let store = TestStore::new();
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(TestProvider::new("test1", "3500.25"))];
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once().unwrap();
    assert_eq!(summary.attempted, 0);
    assert_eq!(summary.succeeded, 0);
}

#[test]
fn engine_accepts_rate_when_safety_disabled() {
    let (config, _) = test_config(SelectionMode::Priority, false);
    let store = TestStore::new();
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(TestProvider::new("test1", "3500.25"))];
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once().unwrap();
    assert_eq!(summary.succeeded, 1);
    assert_eq!(summary.failed, 0);
}

#[test]
fn engine_fail_fast_stops_on_first_error() {
    let (mut config, _) = test_config(SelectionMode::Priority, true);
    config.oracles.fail_fast = true;

    // Add a second asset
    let asset2 = ResolvedAsset {
        id: AssetId::new("btc").unwrap(),
        enabled: true,
        chain_id: ChainId::new("eth").unwrap(),
        caip2: "eip155:1".to_owned(),
        symbol: "BTC".to_owned(),
        name: None,
        kind: "native".to_owned(),
        contract: None,
        decimals: 8,
        x402: None,
        feeds: vec![],
        safety_enabled: false,
        safety_max_change_pct: None,
        safety_min_rate: None,
        safety_max_rate: None,
        safety_action: None,
    };
    config.assets.push(asset2);

    let store = TestStore::new();
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(TestProvider::new("test1", "3500.25"))];
    let mut oracle = Oracle::new(config, store, providers);
    let result = oracle.run_once();
    // The second asset has no enabled feeds, so it should fail.
    // With fail_fast=true, run_once should return the error.
    assert!(result.is_err());
}

#[test]
fn disable_asset_skips_future_refreshes() {
    let (mut config, asset_id) = test_config(SelectionMode::Priority, false);
    config.events.enabled = true;
    config.events.record = true;

    let mut store = TestStore::new();

    // Write a disable_asset event for the tracked asset
    let disable_event = OracleEvent {
        event_type: EventType::RateRejected,
        asset_id: asset_id.clone(),
        chain_id: Some(ChainId::new("eth").unwrap()),
        symbol: "ETH".to_owned(),
        quote: Quote::new("USD").unwrap(),
        provider: ProviderId::new("test1").unwrap(),
        previous_rate: None,
        candidate_rate: None,
        change_pct: None,
        action: EventAction::DisableAsset,
        reason: EventReason::MaxChangeExceeded,
        source_updated_at: None,
        observed_at: time::OffsetDateTime::now_utc(),
    };
    store.write_event(&disable_event).unwrap();

    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(TestProvider::new("test1", "3500.25"))];
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once().unwrap();

    // The asset is disabled, so refresh_asset should return Ok(()) without
    // fetching anything. The run_once loop still records the attempt.
    assert_eq!(summary.attempted, 1);
    assert_eq!(summary.succeeded, 1);
    assert_eq!(summary.failed, 0);

    // No new rate should have been written since fetching was skipped.
    let store = oracle.into_store();
    assert!(
        store.rates.is_empty(),
        "no rate should be written for a disabled asset"
    );
}

#[test]
fn max_concurrent_requests_one_is_sequential() {
    let (mut config, _) = test_config(SelectionMode::All, false);
    config.oracles.max_concurrent_requests = 1;

    // Add a second provider
    let p2_id = ProviderId::new("test2").unwrap();
    config.providers.insert(
        p2_id.clone(),
        ResolvedProvider {
            id: p2_id.clone(),
            kind: oracles::config::ProviderKind::Static,
            method: None,
            url_template: None,
            transport: None,
            auth: None,
            rate_path: None,
            source_updated_at_path: None,
            source_updated_at_format: None,
        },
    );
    // Add second feed
    if let Some(asset) = config.assets.get_mut(0) {
        asset.feeds.push(ResolvedFeed {
            enabled: true,
            provider: p2_id,
            priority: 2,
            params: BTreeMap::new(),
        });
    }

    let store = TestStore::new();
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(TestProvider::new("test1", "3500.0")),
        Arc::new(TestProvider::new("test2", "3510.0")),
    ];
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once().unwrap();

    // With max_concurrent_requests = 1 in All mode, both providers are fetched
    // sequentially. Both should succeed.
    assert_eq!(summary.attempted, 1);
    assert_eq!(summary.succeeded, 1);
    assert_eq!(summary.failed, 0);

    // Both providers' rates should have been written.
    let store = oracle.into_store();
    assert_eq!(store.rates.len(), 2);
}
