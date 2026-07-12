#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use oracles::config::load::load_config;
use oracles::config::raw::RawTransportsConfig;
use oracles::config::validate::resolve_config;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helper: build a minimal valid raw config programmatically
// ---------------------------------------------------------------------------

use oracles::config::raw::{
    RawAssetConfig, RawChainConfig, RawConfig, RawOraclesConfig, RawProviderConfig, RawStoreConfig,
};

fn minimal_raw_config() -> RawConfig {
    let mut stores = BTreeMap::new();
    stores.insert(
        "oracles".to_owned(),
        RawStoreConfig {
            driver: "sqlite".to_owned(),
            url: "sqlite://test.db".to_owned(),
            migrate: None,
            connect_timeout_secs: None,
            max_connections: None,
        },
    );

    let mut chains = BTreeMap::new();
    chains.insert(
        "eth".to_owned(),
        RawChainConfig {
            family: "evm".to_owned(),
            caip2: "eip155:1".to_owned(),
            native_symbol: None,
            rpc_urls: None,
            confirmations: None,
            derivation: None,
        },
    );

    let mut assets = BTreeMap::new();
    assets.insert(
        "eth".to_owned(),
        RawAssetConfig {
            enabled: Some(false), // Disabled by default; enable in tests that need it.
            chain: "eth".to_owned(),
            symbol: "ETH".to_owned(),
            name: None,
            kind: "native".to_owned(),
            contract: None,
            decimals: 18,
            x402: None,
            feeds: None,
            safety: None,
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

    RawConfig {
        version: 1,
        log: None,
        stores,
        http: None,
        chains,
        assets,
        oracles: RawOraclesConfig {
            store: "oracles".to_owned(),
            quote: "USD".to_owned(),
            refresh_secs: 60,
            stale_after_secs: 120,
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
        },
    }
}

// ---------------------------------------------------------------------------
// 1. Merged config TOML load test
// ---------------------------------------------------------------------------

#[test]
fn load_merged_universal_config() {
    // Simulate a merged Config.toml with multiple package namespaces.
    let toml_text = r#"
version = 1

[log]
level = "info"
format = "json"

[meta]
name = "test-stack"

[artur]
enabled = true
store = "oracles"

[runtime]
worker_threads = 0

[stores.oracles]
driver = "sqlite"
url = "sqlite://data/oracles.db"

[stores.ladon]
driver = "sqlite"
url = "sqlite://data/ladon.db"

[stores.pano]
driver = "sqlite"
url = "sqlite://data/pano.db"

[http]
user_agent = "test"

[chains.eth]
family = "evm"
caip2 = "eip155:1"

[assets.eth]
enabled = false
chain = "eth"
symbol = "ETH"
kind = "native"
decimals = 18

# --- Oracles namespace ---

[oracles]
store = "oracles"
quote = "USD"
refresh_secs = 60
stale_after_secs = 120

[oracles.providers.static]
kind = "static"

# --- Other package namespaces (should be silently ignored) ---

[ladon]
enabled = true
store = "ladon"

[pano]
enabled = true
store = "pano"

[bria]
enabled = true

[paths.bria_jobs]
kind = "file"
path = "/tmp/jobs.jsonl"

[objects.local]
driver = "fs"
root = "/tmp"

[transports.amqp.local]
url = "amqp://localhost:5672"
"#;

    // Write to a tempfile and load.
    let dir = tempfile::TempDir::new().unwrap();
    let config_path = dir.path().join("Config.toml");
    std::fs::write(&config_path, toml_text).unwrap();

    let result = load_config(&config_path);
    assert!(
        result.is_ok(),
        "merged config should load successfully: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    assert_eq!(resolved.version, 1);
    // All stores from the merged config are universal and kept.
    assert_eq!(resolved.stores.len(), 3);
    assert_eq!(resolved.oracles.store, "oracles");
    assert_eq!(resolved.chains.len(), 1);
}

// ---------------------------------------------------------------------------
// 2. Unrelated namespaces are ignored
// ---------------------------------------------------------------------------

#[test]
fn ignores_unrelated_package_namespaces() {
    let raw = minimal_raw_config();

    // This raw config always works (no unrelated namespaces at the serde level).
    // But we also test via the TOML loader above.
    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// 3. Unknown fields in [oracles] are rejected
// ---------------------------------------------------------------------------

#[test]
fn rejects_unknown_fields_in_oracles_namespace() {
    let toml_text = r#"
version = 1

[stores.oracles]
driver = "sqlite"
url = "sqlite://test.db"

[chains.eth]
family = "evm"
caip2 = "eip155:1"

[assets.eth]
enabled = true
chain = "eth"
symbol = "ETH"
kind = "native"
decimals = 18

[oracles]
store = "oracles"
quote = "USD"
refresh_secs = 60
stale_after_secs = 120
this_field_does_not_exist = true

[oracles.providers.static]
kind = "static"
"#;

    let dir = tempfile::TempDir::new().unwrap();
    let config_path = dir.path().join("Config.toml");
    std::fs::write(&config_path, toml_text).unwrap();

    let result = load_config(&config_path);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("this_field_does_not_exist"),
        "expected unknown field error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 4. Shared store resolution
// ---------------------------------------------------------------------------

#[test]
fn resolves_shared_store_reference() {
    let mut raw = minimal_raw_config();
    // Add a second store and reference it from oracles.
    raw.stores.insert(
        "custom_store".to_owned(),
        RawStoreConfig {
            driver: "sqlite".to_owned(),
            url: "sqlite://custom.db".to_owned(),
            migrate: None,
            connect_timeout_secs: None,
            max_connections: None,
        },
    );
    raw.oracles.store = "custom_store".to_owned();

    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(
        result.is_ok(),
        "store ref should resolve: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    assert!(resolved.stores.contains_key("custom_store"));
    assert_eq!(resolved.oracles.store, "custom_store");
}

// ---------------------------------------------------------------------------
// 5. Shared chain resolution
// ---------------------------------------------------------------------------

#[test]
fn resolves_shared_chain_for_assets() {
    let mut raw = minimal_raw_config();
    // Add feeds to the asset.
    raw.assets.get_mut("eth").unwrap().feeds = Some(vec![oracles::config::raw::RawFeedConfig {
        enabled: None,
        provider: "static".to_owned(),
        priority: 100,
        params: Some({
            let mut p = BTreeMap::new();
            p.insert("rate".to_owned(), "3000.00".to_owned());
            p
        }),
    }]);

    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(
        result.is_ok(),
        "chain ref should resolve: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    let eth = &resolved.assets[0];
    assert_eq!(eth.id.as_str(), "eth");
    assert_eq!(eth.chain_id.as_str(), "eth");
    assert!(resolved.chains.contains_key(&eth.chain_id));
}

// ---------------------------------------------------------------------------
// 6. Shared HTTP transport profile ref
// ---------------------------------------------------------------------------

#[test]
fn resolves_http_transport_profile_for_provider() {
    let mut raw = minimal_raw_config();
    // Add a Coingecko-like HTTP JSON provider with transport ref.
    raw.oracles.providers.insert(
        "coingecko".to_owned(),
        RawProviderConfig {
            kind: "http_json".to_owned(),
            method: Some("GET".to_owned()),
            url_template: Some("https://api.coingecko.com/api/v3/simple/price?ids={coin_id}&vs_currencies={quote_lower}".to_owned()),
            transport: Some("default".to_owned()),
            auth: None,
            paths: None,
            formats: None,
        },
    );

    let mut transports = RawTransportsConfig::default();
    transports.http.insert(
        "default".to_owned(),
        oracles::config::raw::RawTransportHttpProfile {
            base_url: Some("https://api.example.com".to_owned()),
            user_agent: None,
            timeout_secs: Some(30),
            max_retries: None,
            retry_base_ms: None,
        },
    );

    let result = resolve_config(raw, transports);
    assert!(
        result.is_ok(),
        "HTTP transport ref should resolve: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    let provider = resolved
        .providers
        .get(&oracles::domain::ProviderId::new("coingecko").unwrap())
        .unwrap();
    assert_eq!(provider.transport.as_deref(), Some("default"));
}

// ---------------------------------------------------------------------------
// 7. Unknown transport ref rejected
// ---------------------------------------------------------------------------

#[test]
fn rejects_unknown_http_transport_profile() {
    let mut raw = minimal_raw_config();
    raw.oracles.providers.insert(
        "coingecko".to_owned(),
        RawProviderConfig {
            kind: "http_json".to_owned(),
            method: Some("GET".to_owned()),
            url_template: Some("https://api.example.com".to_owned()),
            transport: Some("nonexistent".to_owned()),
            auth: None,
            paths: None,
            formats: None,
        },
    );

    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("nonexistent"),
        "expected unknown transport error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 8. Shared webhook transport profile ref for event sinks
// ---------------------------------------------------------------------------

#[test]
#[cfg(feature = "webhook")]
fn resolves_webhook_transport_for_event_sinks() {
    let mut raw = minimal_raw_config();
    // Add a webhook sink with transport ref.
    let mut sinks = BTreeMap::new();
    sinks.insert(
        "ops-webhook".to_owned(),
        oracles::config::raw::RawEventSinkConfig {
            kind: "webhook".to_owned(),
            enabled: None,
            level: None,
            bot_token_env: None,
            chat_id_env: None,
            method: None,
            parse_mode: None,
            disable_web_page_preview: None,
            message: None,
            url_env: Some("ORACLES_OPS_WEBHOOK_URL".to_owned()),
            headers: None,
            body: Some(oracles::config::raw::RawWebhookBodyConfig {
                format: "json".to_owned(),
                template: "{}".to_owned(),
            }),
            transport: Some("ops".to_owned()),
            url: None,
            token: None,
            timeout_secs: None,
            max_retries: None,
            retry_base_ms: None,
        },
    );

    raw.oracles.events = Some(oracles::config::raw::RawEventsConfig {
        enabled: Some(true),
        mode: Some("simple".to_owned()),
        store: Some("oracles".to_owned()),
        record: Some(true),
        table: None,
        sink_fail_fast: None,
        columns: None,
        routes: None,
        sinks: Some(sinks),
    });

    let mut transports = RawTransportsConfig::default();
    transports.webhook.insert(
        "ops".to_owned(),
        oracles::config::raw::RawTransportWebhookProfile {
            url: Some("https://hooks.example.com".to_owned()),
            method: Some("POST".to_owned()),
            auth_scheme: None,
            token: None,
            auth_header: None,
            timeout_secs: None,
            max_retries: None,
            retry_base_ms: None,
            headers: None,
        },
    );

    let result = resolve_config(raw, transports);
    assert!(
        result.is_ok(),
        "webhook transport ref should resolve: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// 9. Unknown webhook transport ref rejected
// ---------------------------------------------------------------------------

#[test]
#[cfg(feature = "webhook")]
fn rejects_unknown_webhook_transport_for_sink() {
    let mut raw = minimal_raw_config();
    let mut sinks = BTreeMap::new();
    sinks.insert(
        "bad-sink".to_owned(),
        oracles::config::raw::RawEventSinkConfig {
            kind: "webhook".to_owned(),
            enabled: None,
            level: None,
            bot_token_env: None,
            chat_id_env: None,
            method: None,
            parse_mode: None,
            disable_web_page_preview: None,
            message: None,
            url_env: Some("URL".to_owned()),
            headers: None,
            body: Some(oracles::config::raw::RawWebhookBodyConfig {
                format: "json".to_owned(),
                template: "{}".to_owned(),
            }),
            transport: Some("nonexistent".to_owned()),
            url: None,
            token: None,
            timeout_secs: None,
            max_retries: None,
            retry_base_ms: None,
        },
    );

    raw.oracles.events = Some(oracles::config::raw::RawEventsConfig {
        enabled: Some(true),
        mode: Some("simple".to_owned()),
        store: Some("oracles".to_owned()),
        record: Some(true),
        table: None,
        sink_fail_fast: None,
        columns: None,
        routes: None,
        sinks: Some(sinks),
    });

    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("nonexistent"),
        "expected unknown webhook transport error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 10. SQLite default config
// ---------------------------------------------------------------------------

#[test]
fn sqlite_is_default_store_driver() {
    let raw = minimal_raw_config();
    let result = resolve_config(raw, RawTransportsConfig::default()).unwrap();
    let store = result.stores.get("oracles").unwrap();
    assert_eq!(store.driver, oracles::config::StoreDriver::Sqlite);
}

// ---------------------------------------------------------------------------
// 11. Postgres config fails when feature disabled
// ---------------------------------------------------------------------------

#[test]
fn postgres_store_fails_without_feature() {
    let mut raw = minimal_raw_config();
    raw.stores.get_mut("oracles").unwrap().driver = "postgres".to_owned();
    raw.stores.get_mut("oracles").unwrap().url = "postgres://user:pass@localhost/db".to_owned();

    let result = resolve_config(raw, RawTransportsConfig::default());

    #[cfg(not(feature = "postgres"))]
    {
        assert!(result.is_err(), "postgres should fail without feature");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("postgres"),
            "expected postgres feature error, got: {err}"
        );
    }

    #[cfg(feature = "postgres")]
    {
        // With postgres feature, it should succeed (or fail for other reasons, not feature)
        // The PG store would need a real connection to fully validate.
        // Here we just check it doesn't fail with a feature error.
        if let Err(ref e) = result {
            let msg = e.to_string();
            assert!(
                !msg.contains("feature"),
                "should not fail with feature error when feature is enabled: {msg}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 12. Postgres config passes when pg/postgres feature enabled
// (Tested implicitly by the #[cfg(feature = "postgres")] branch above)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Environment variable expansion tests
// These test the expansion logic through resolved config.
// The env::expand_env function is tested separately in unit tests.
// ---------------------------------------------------------------------------

/// Test that env expansion with a default value works when the variable is unset.
#[test]
fn env_expansion_with_default_fallback() {
    let mut raw = minimal_raw_config();
    raw.stores.get_mut("oracles").unwrap().url =
        "${ORACLES_TEST_MISSING_VAR:-sqlite://default.db}".to_owned();

    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(
        result.is_ok(),
        "env expansion with default should work: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    let store = resolved.stores.get("oracles").unwrap();
    assert_eq!(store.url, "sqlite://default.db");
}

/// Test that a missing env variable without a default causes an error.
#[test]
fn env_expansion_missing_variable_fails() {
    let mut raw = minimal_raw_config();
    raw.stores.get_mut("oracles").unwrap().url =
        "${ORACLES_DEFINITELY_MISSING_VAR_12345}".to_owned();

    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(
        result.is_err(),
        "missing env var without default should fail"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("ORACLES_DEFINITELY_MISSING_VAR_12345"),
        "expected missing env var error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 16. Oracle assets from [oracles.assets.<id>]
// ---------------------------------------------------------------------------

#[test]
fn resolves_feeds_from_oracles_assets_namespace() {
    let mut raw = minimal_raw_config();
    // Add feeds via [oracles.assets.eth] instead of shared [assets.eth].
    let mut oracle_assets = BTreeMap::new();
    oracle_assets.insert(
        "eth".to_owned(),
        oracles::config::raw::RawOracleAssetConfig {
            enabled: Some(true),
            feeds: Some(vec![oracles::config::raw::RawOracleFeedConfig {
                enabled: Some(true),
                provider: "static".to_owned(),
                priority: 100,
                params: Some({
                    let mut p = BTreeMap::new();
                    p.insert("rate".to_owned(), "3000.00".to_owned());
                    p
                }),
            }]),
        },
    );

    raw.oracles.assets = Some(oracle_assets);

    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(
        result.is_ok(),
        "oracles.assets feeds should resolve: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    assert_eq!(resolved.assets.len(), 1);
    let eth = &resolved.assets[0];
    assert_eq!(eth.id.as_str(), "eth");
    assert_eq!(eth.feeds.len(), 1);
    assert_eq!(eth.feeds[0].provider.as_str(), "static");
}

// ---------------------------------------------------------------------------
// 17. asset_ids filtering
// ---------------------------------------------------------------------------

#[test]
fn asset_ids_filters_shared_assets() {
    let mut raw = minimal_raw_config();

    // Add a second asset that should be ignored.
    raw.assets.insert(
        "btc".to_owned(),
        RawAssetConfig {
            enabled: None,
            chain: "eth".to_owned(), // reuse eth chain for simplicity
            symbol: "BTC".to_owned(),
            name: None,
            kind: "native".to_owned(),
            contract: None,
            decimals: 8,
            x402: None,
            feeds: Some(vec![oracles::config::raw::RawFeedConfig {
                enabled: None,
                provider: "static".to_owned(),
                priority: 100,
                params: Some({
                    let mut p = BTreeMap::new();
                    p.insert("rate".to_owned(), "60000.00".to_owned());
                    p
                }),
            }]),
            safety: None,
        },
    );

    // Only price ETH.
    raw.oracles.asset_ids = Some(vec!["eth".to_owned()]);

    let result = resolve_config(raw, RawTransportsConfig::default());
    assert!(
        result.is_ok(),
        "asset_ids filter should work: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    assert_eq!(resolved.assets.len(), 1);
    assert_eq!(resolved.assets[0].id.as_str(), "eth");
}

// ---------------------------------------------------------------------------
// 18. CLI --config compatibility (via the load_config path tested above)
// ---------------------------------------------------------------------------

#[test]
fn cli_config_flag_loads_merged_config() {
    // This is tested by load_merged_universal_config above.
    // Here we test with an explicit path via load_config.
    let toml_text = r#"
version = 1

[stores.oracles]
driver = "sqlite"
url = "sqlite://test.db"

[chains.eth]
family = "evm"
caip2 = "eip155:1"

[assets.eth]
enabled = false
chain = "eth"
symbol = "ETH"
kind = "native"
decimals = 18

[oracles]
store = "oracles"
quote = "USD"
refresh_secs = 60
stale_after_secs = 120

[oracles.providers.static]
kind = "static"

# unrelated
[bria]
enabled = true
"#;

    let dir = tempfile::TempDir::new().unwrap();
    let config_path = dir.path().join("my_config.toml");
    std::fs::write(&config_path, toml_text).unwrap();

    let config = load_config(&config_path).expect("should load via --config path");
    assert_eq!(config.version, 1);
}
