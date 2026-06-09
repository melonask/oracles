#![allow(missing_docs)]
#![cfg(feature = "sqlite")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use oracles::config::{
    BootstrapAction, EventMode, ProviderKind, ResolvedConfig, ResolvedConsensusConfig,
    ResolvedEventColumns, ResolvedEventsConfig, ResolvedHttpConfig, ResolvedOraclesConfig,
    ResolvedOutboxColumns, ResolvedOutboxConfig, ResolvedProvider, ResolvedRateColumns,
    ResolvedRateTableConfig, ResolvedSafetyConfig, ResolvedStoreConfig, SelectionMode, StoreDriver,
    WriteMode,
};
use oracles::domain::{
    AssetId, ChainId, EventAction, EventReason, EventType, OracleEvent, ProviderId, Quote,
    RateAmount, RateRecord,
};
use oracles::store::RateStore;
use oracles::store::sqlite::SqliteRateStore;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use time::{Duration, OffsetDateTime};

#[test]
fn open_in_memory_and_read_write_rate() -> oracles::Result<()> {
    let config = minimal_config("sqlite::memory:")?;
    let mut store = SqliteRateStore::open(&config)?;

    let asset_id = AssetId::new("eth")?;
    let quote = Quote::new("USD")?;

    // Initially no rate
    let prev = store.last_accepted_rate(&asset_id, &quote)?;
    assert!(prev.is_none(), "expected no previous rate in fresh store");

    // Write a rate
    let now = OffsetDateTime::now_utc();
    let record = RateRecord {
        asset_id: asset_id.clone(),
        chain_id: ChainId::new("eth")?,
        caip2: "eip155:1".to_owned(),
        symbol: "ETH".to_owned(),
        quote: quote.clone(),
        provider: ProviderId::new("static")?,
        rate: RateAmount::parse("3500.25")?,
        source_updated_at: Some(now),
        observed_at: now,
        expires_at: now + Duration::seconds(300),
    };

    store.write_accepted_rate(&record)?;

    // Read it back
    let found = store.last_accepted_rate(&asset_id, &quote)?;
    assert!(found.is_some(), "expected rate after write");
    let found = found.unwrap();
    assert_eq!(found.asset_id, asset_id);
    assert_eq!(found.rate.to_string(), "3500.25");

    Ok(())
}

#[test]
fn append_mode_writes_multiple_rows_and_returns_latest() -> oracles::Result<()> {
    let config = config_with_write_mode("sqlite::memory:", WriteMode::Append)?;
    let mut store = SqliteRateStore::open(&config)?;

    let asset_id = AssetId::new("eth")?;
    let quote = Quote::new("USD")?;
    let provider = ProviderId::new("static")?;

    let now = OffsetDateTime::now_utc();

    // Write first rate
    let record1 = RateRecord {
        asset_id: asset_id.clone(),
        chain_id: ChainId::new("eth")?,
        caip2: "eip155:1".to_owned(),
        symbol: "ETH".to_owned(),
        quote: quote.clone(),
        provider: provider.clone(),
        rate: RateAmount::parse("3500.25")?,
        source_updated_at: Some(now),
        observed_at: now,
        expires_at: now + Duration::seconds(300),
    };
    store.write_accepted_rate(&record1)?;

    // Write second rate (same asset/quote/provider)
    let later = now + Duration::seconds(60);
    let record2 = RateRecord {
        asset_id: asset_id.clone(),
        chain_id: ChainId::new("eth")?,
        caip2: "eip155:1".to_owned(),
        symbol: "ETH".to_owned(),
        quote: quote.clone(),
        provider: provider.clone(),
        rate: RateAmount::parse("3600.00")?,
        source_updated_at: Some(later),
        observed_at: later,
        expires_at: later + Duration::seconds(300),
    };
    store.write_accepted_rate(&record2)?;

    // In append mode, last_accepted_rate should return the most recent row.
    let found = store.last_accepted_rate(&asset_id, &quote)?;
    assert!(found.is_some(), "expected rate after two appends");
    let found = found.unwrap();
    assert_eq!(found.rate.to_string(), "3600.00");
    assert_eq!(found.provider, provider);

    Ok(())
}

#[test]
fn custom_column_names_work_for_rates() -> oracles::Result<()> {
    let mut config = minimal_config("sqlite::memory:")?;
    // Override column names: "rate" -> "price_text", "provider" -> "source"
    config.oracles.table.columns.rate = "price_text".to_owned();
    config.oracles.table.columns.provider = "source".to_owned();

    let mut store = SqliteRateStore::open(&config)?;

    let asset_id = AssetId::new("eth")?;
    let quote = Quote::new("USD")?;

    let now = OffsetDateTime::now_utc();
    let record = RateRecord {
        asset_id: asset_id.clone(),
        chain_id: ChainId::new("eth")?,
        caip2: "eip155:1".to_owned(),
        symbol: "ETH".to_owned(),
        quote: quote.clone(),
        provider: ProviderId::new("static")?,
        rate: RateAmount::parse("3500.25")?,
        source_updated_at: Some(now),
        observed_at: now,
        expires_at: now + Duration::seconds(300),
    };

    store.write_accepted_rate(&record)?;

    let found = store.last_accepted_rate(&asset_id, &quote)?;
    assert!(found.is_some(), "expected rate with custom column names");
    let found = found.unwrap();
    assert_eq!(found.rate.to_string(), "3500.25");
    assert_eq!(found.provider.as_str(), "static");

    Ok(())
}

#[test]
fn custom_column_names_work_for_events() -> oracles::Result<()> {
    let mut config = minimal_config("sqlite::memory:")?;
    config.events.enabled = true;
    config.events.record = true;
    // Override event column names: "event_type" -> "kind", "action" -> "disposition"
    config.events.columns.event_type = "kind".to_owned();
    config.events.columns.action = "disposition".to_owned();

    let mut store = SqliteRateStore::open(&config)?;

    let now = OffsetDateTime::now_utc();
    let event = OracleEvent {
        event_type: EventType::RateAnomaly,
        asset_id: AssetId::new("eth")?,
        chain_id: Some(ChainId::new("eth")?),
        symbol: "ETH".to_owned(),
        quote: Quote::new("USD")?,
        provider: ProviderId::new("static")?,
        previous_rate: Some(RateAmount::parse("3400")?),
        candidate_rate: Some(RateAmount::parse("3500")?),
        change_pct: Some(Decimal::new(294, 2)), // 2.94%
        action: EventAction::Alert,
        reason: EventReason::MaxChangeExceeded,
        source_updated_at: Some(now),
        observed_at: now,
    };

    store.write_event(&event)?;

    // Verify the event was stored by checking cooldown (has_recent_event)
    let has_recent = store.has_recent_event(
        &AssetId::new("eth")?,
        &ProviderId::new("static")?,
        &EventType::RateAnomaly,
        &event.reason,
        3600,
    )?;
    assert!(has_recent, "event should be found with custom column names");

    Ok(())
}

#[test]
fn disable_asset_event_is_detected() -> oracles::Result<()> {
    let mut config = minimal_config("sqlite::memory:")?;
    config.events.enabled = true;
    config.events.record = true;

    let mut store = SqliteRateStore::open(&config)?;

    let now = OffsetDateTime::now_utc();
    let event = OracleEvent {
        event_type: EventType::RateRejected,
        asset_id: AssetId::new("eth")?,
        chain_id: Some(ChainId::new("eth")?),
        symbol: "ETH".to_owned(),
        quote: Quote::new("USD")?,
        provider: ProviderId::new("static")?,
        previous_rate: None,
        candidate_rate: None,
        change_pct: None,
        action: EventAction::DisableAsset,
        reason: EventReason::MaxChangeExceeded,
        source_updated_at: None,
        observed_at: now,
    };

    store.write_event(&event)?;

    let is_disabled = store.has_recent_disable_event(&AssetId::new("eth")?)?;
    assert!(is_disabled, "disable_asset event should be detected");

    // A different asset should not be disabled
    let other_disabled = store.has_recent_disable_event(&AssetId::new("btc")?)?;
    assert!(!other_disabled, "other asset should not be disabled");

    Ok(())
}

fn config_with_write_mode(db_url: &str, write_mode: WriteMode) -> oracles::Result<ResolvedConfig> {
    let mut config = minimal_config(db_url)?;
    config.oracles.table.write_mode = write_mode;
    Ok(config)
}

fn minimal_config(db_url: &str) -> oracles::Result<ResolvedConfig> {
    let store_id = "oracles".to_owned();
    let provider_id = ProviderId::new("static")?;
    let quote = Quote::new("USD")?;

    let mut stores = BTreeMap::new();
    stores.insert(
        store_id.clone(),
        ResolvedStoreConfig {
            driver: StoreDriver::Sqlite,
            url: db_url.to_owned(),
            migrate: true,
            connect_timeout_secs: 10,
            max_connections: 1,
        },
    );

    let mut providers = BTreeMap::new();
    providers.insert(
        provider_id.clone(),
        ResolvedProvider {
            id: provider_id,
            kind: ProviderKind::Static,
            method: None,
            url_template: None,
            auth: None,
            rate_path: None,
            source_updated_at_path: None,
            source_updated_at_format: None,
        },
    );

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
        assets: Vec::new(),
        oracles: ResolvedOraclesConfig {
            store: store_id,
            quote,
            refresh_secs: 180,
            stale_after_secs: 300,
            max_source_age_secs: None,
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
            compare_against: oracles::config::CompareAgainst::LastAccepted,
            default_action: EventAction::Quarantine,
            max_change_pct: Decimal::new(50, 0),
            min_rate: None,
            max_rate: None,
            max_source_age: None,
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
            routes: Vec::new(),
            sinks: BTreeMap::new(),
        },
        outbox: ResolvedOutboxConfig {
            enabled: false,
            store: "oracles".to_owned(),
            table: "oracle_outbox".to_owned(),
            dispatch_interval_secs: 10,
            max_retries: 5,
            retry_backoff_secs: 30,
            request_timeout_secs: 10,
            columns: ResolvedOutboxColumns::defaults(),
        },
        providers,
    })
}
