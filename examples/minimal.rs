//! Minimal example: create an oracle with a static provider, run once, print summary.
//!
//! This example constructs a [`ResolvedConfig`] programmatically (without TOML),
//! opens an in-memory SQLite store, registers a [`StaticProvider`] that returns
//! a fixed ETH/USD rate of 3500.00, and runs a single refresh cycle.
//!
//! ```sh
//! cargo run --example minimal
//! ```

use std::collections::BTreeMap;
use std::sync::Arc;

use rust_decimal::Decimal;
use time::Duration;

use oracles::Result;
use oracles::config::{
    BootstrapAction, CompareAgainst, EventMode, ProviderKind, ResolvedAsset, ResolvedChain,
    ResolvedConfig, ResolvedConsensusConfig, ResolvedEventColumns, ResolvedEventsConfig,
    ResolvedFeed, ResolvedHttpConfig, ResolvedLogConfig, ResolvedOraclesConfig,
    ResolvedOutboxColumns, ResolvedOutboxConfig, ResolvedProvider, ResolvedRateColumns,
    ResolvedRateTableConfig, ResolvedSafetyConfig, ResolvedStoreConfig, SelectionMode, StoreDriver,
    WriteMode,
};
use oracles::domain::{AssetId, ChainId, EventAction, ProviderId, Quote};
use oracles::engine::Oracle;
use oracles::provider::{Provider, static_provider::StaticProvider};
use oracles::store::sqlite::SqliteRateStore;

fn run() -> Result<()> {
    // ------------------------------------------------------------------
    // 1. Build a ResolvedConfig manually (no TOML file needed).
    // ------------------------------------------------------------------

    // -- Store config: in-memory SQLite --
    let mut stores = BTreeMap::new();
    stores.insert(
        "oracles".to_owned(),
        ResolvedStoreConfig {
            driver: StoreDriver::Sqlite,
            url: "sqlite::memory:".to_owned(),
            migrate: true,
            connect_timeout_secs: 10,
            max_connections: 1,
        },
    );

    // -- Chain config: Ethereum mainnet --
    let eth_chain_id = ChainId::new("eth")?;
    let mut chains = BTreeMap::new();
    chains.insert(
        eth_chain_id.clone(),
        ResolvedChain {
            id: eth_chain_id.clone(),
            family: "evm".to_owned(),
            caip2: "eip155:1".to_owned(),
            native_symbol: Some("ETH".to_owned()),
            rpc_urls: vec![],
            confirmations: 0,
        },
    );

    // -- Provider config: a single static provider --
    let provider_id = ProviderId::new("static-eth")?;
    let mut providers = BTreeMap::new();
    providers.insert(
        provider_id.clone(),
        ResolvedProvider {
            id: provider_id.clone(),
            kind: ProviderKind::Static,
            method: None,
            url_template: None,
            transport: None,
            auth: None,
            rate_path: None,
            source_updated_at_path: None,
            source_updated_at_format: None,
        },
    );

    // -- Asset config: ETH with a static feed at $3,500 --
    let asset_id = AssetId::new("eth")?;

    let mut feed_params = BTreeMap::new();
    feed_params.insert("rate".to_owned(), "3500.00".to_owned());

    let assets = vec![ResolvedAsset {
        id: asset_id,
        enabled: true,
        chain_id: eth_chain_id,
        caip2: "eip155:1".to_owned(),
        symbol: "ETH".to_owned(),
        name: Some("Ether".to_owned()),
        kind: "native".to_owned(),
        contract: None,
        decimals: 18,
        x402: None,
        feeds: vec![ResolvedFeed {
            enabled: true,
            provider: provider_id.clone(),
            priority: 0,
            params: feed_params,
        }],
        safety_enabled: false,
        safety_max_change_pct: None,
        safety_min_rate: None,
        safety_max_rate: None,
        safety_action: None,
    }];

    // -- Assemble the full ResolvedConfig --
    let config = ResolvedConfig {
        version: 1,
        log: ResolvedLogConfig {
            level: "info".to_owned(),
            format: "json".to_owned(),
        },
        stores,
        http: ResolvedHttpConfig {
            user_agent: "oracles-minimal-example/0.1".to_owned(),
            request_timeout_secs: 15,
            max_retries: 3,
            retry_backoff_ms: 500,
        },
        chains,
        assets,
        oracles: ResolvedOraclesConfig {
            store: "oracles".to_owned(),
            quote: Quote::new("USD")?,
            refresh_secs: 30,
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
            enabled: false,
            compare_against: CompareAgainst::LastAccepted,
            default_action: EventAction::Alert,
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
            routes: vec![],
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
    };

    // ------------------------------------------------------------------
    // 2. Open the in-memory SQLite store.
    // ------------------------------------------------------------------
    let store = SqliteRateStore::open(&config)?;

    // ------------------------------------------------------------------
    // 3. Build the static provider.
    // ------------------------------------------------------------------
    let static_provider = StaticProvider::new(provider_id);
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(static_provider)];

    // ------------------------------------------------------------------
    // 4. Create the oracle engine and run once.
    // ------------------------------------------------------------------
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once()?;

    // ------------------------------------------------------------------
    // 5. Print the refresh summary.
    // ------------------------------------------------------------------
    eprintln!("--- Refresh Summary ---");
    eprintln!("Attempted: {}", summary.attempted);
    eprintln!("Succeeded: {}", summary.succeeded);
    eprintln!("Failed:    {}", summary.failed);

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
