use std::collections::BTreeMap;

#[cfg(feature = "config-toml")]
use serde::Deserialize;

/// Raw top-level configuration as deserialized from TOML.
///
/// This is the "untrusted" input that must be validated and resolved into a
/// [`crate::config::ResolvedConfig`] before use.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawConfig {
    /// Config schema version (must be `1`).
    pub version: u32,
    /// Optional logging configuration.
    pub log: Option<RawLogConfig>,
    /// Store backend definitions.
    pub stores: BTreeMap<String, RawStoreConfig>,
    /// Optional HTTP client settings.
    pub http: Option<RawHttpConfig>,
    /// Blockchain network definitions.
    pub chains: BTreeMap<String, RawChainConfig>,
    /// Asset definitions keyed by asset ID.
    pub assets: BTreeMap<String, RawAssetConfig>,
    /// Oracle engine settings.
    pub oracles: RawOraclesConfig,
}

/// Raw logging configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawLogConfig {
    /// Log level (`"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`).
    pub level: Option<String>,
    /// Log format (`"json"`, `"pretty"`, `"compact"`).
    pub format: Option<String>,
}

/// Raw store backend configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawStoreConfig {
    /// Driver name (`"sqlite"` or `"postgres"`).
    pub driver: String,
    /// Connection URL.
    pub url: String,
    /// Whether to run migrations on open.
    pub migrate: Option<bool>,
    /// Connection timeout in seconds.
    pub connect_timeout_secs: Option<u64>,
    /// Maximum connections in the pool.
    pub max_connections: Option<u32>,
}

/// Raw HTTP client configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawHttpConfig {
    /// The `User-Agent` header value.
    pub user_agent: Option<String>,
    /// Request timeout in seconds.
    pub request_timeout_secs: Option<u64>,
    /// Maximum retry attempts.
    pub max_retries: Option<u32>,
    /// Retry backoff delay in milliseconds.
    pub retry_backoff_ms: Option<u64>,
}

/// Raw blockchain network configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawChainConfig {
    /// Chain family (`"evm"`, `"svm"`, etc.).
    pub family: String,
    /// CAIP-2 identifier.
    pub caip2: String,
    /// Native currency symbol.
    pub native_symbol: Option<String>,
    /// RPC endpoint URLs.
    pub rpc_urls: Option<Vec<String>>,
    /// Required block confirmations.
    pub confirmations: Option<u32>,
}

/// Raw asset configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawAssetConfig {
    /// Whether the asset is enabled.
    pub enabled: Option<bool>,
    /// Chain ID this asset belongs to.
    pub chain: String,
    /// Asset symbol (e.g., `"ETH"`).
    pub symbol: String,
    /// Human-readable name.
    pub name: Option<String>,
    /// Asset kind (`"native"`, `"erc20"`, etc.).
    pub kind: String,
    /// Contract address (for token assets).
    pub contract: Option<String>,
    /// Number of decimals.
    pub decimals: u8,
    /// Optional X402 configuration.
    pub x402: Option<RawX402Config>,
    /// Provider feed definitions.
    pub feeds: Option<Vec<RawFeedConfig>>,
    /// Per-asset safety overrides.
    pub safety: Option<RawAssetSafetyConfig>,
}

/// Raw X402 (HTTP 402 Payment Required) configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawX402Config {
    /// Whether X402 is enabled.
    pub enabled: Option<bool>,
    /// Asset contract address.
    pub asset_address: String,
    /// Transfer method.
    pub transfer_method: String,
}

/// Raw provider feed configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawFeedConfig {
    /// Whether this feed is enabled.
    pub enabled: Option<bool>,
    /// Provider name.
    pub provider: String,
    /// Feed priority (higher = tried first).
    pub priority: i32,
    /// Provider-specific parameters.
    pub params: Option<BTreeMap<String, String>>,
}

/// Raw per-asset safety configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawAssetSafetyConfig {
    /// Whether safety checks are enabled for this asset.
    pub enabled: Option<bool>,
    /// Maximum percent change.
    pub max_change_pct: Option<String>,
    /// Minimum acceptable rate.
    pub min_rate: Option<String>,
    /// Maximum acceptable rate.
    pub max_rate: Option<String>,
    /// Safety action.
    pub action: Option<String>,
}

/// Raw oracle engine configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawOraclesConfig {
    /// Store backend name.
    pub store: String,
    /// Quote currency (e.g., `"USD"`).
    pub quote: String,
    /// Refresh interval in seconds.
    pub refresh_secs: u64,
    /// Stale-after duration in seconds.
    pub stale_after_secs: u64,
    /// Maximum source data age in seconds.
    pub max_source_age_secs: Option<u64>,
    /// Maximum concurrent provider requests.
    pub max_concurrent_requests: Option<usize>,
    /// Whether to abort on first failure.
    pub fail_fast: Option<bool>,
    /// Rate selection strategy.
    pub selection: Option<String>,
    /// Rate table configuration.
    pub table: Option<RawRateTableConfig>,
    /// Global safety settings.
    pub safety: Option<RawSafetyConfig>,
    /// Event system configuration.
    pub events: Option<RawEventsConfig>,
    /// Outbox configuration.
    pub outbox: Option<RawOutboxConfig>,
    /// Provider definitions.
    pub providers: BTreeMap<String, RawProviderConfig>,
}

/// Raw rate table configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawRateTableConfig {
    /// Table name.
    pub name: String,
    /// Write mode (`"upsert"` or `"append"`).
    pub write_mode: Option<String>,
    /// Column name overrides.
    pub columns: Option<BTreeMap<String, String>>,
}

/// Raw global safety configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawSafetyConfig {
    /// Whether the safety engine is enabled globally.
    pub enabled: Option<bool>,
    /// Baseline for percent-change comparison.
    pub compare_against: Option<String>,
    /// Default safety action.
    pub default_action: Option<String>,
    /// Maximum percent change.
    pub max_change_pct: Option<String>,
    /// Minimum acceptable rate.
    pub min_rate: Option<String>,
    /// Maximum acceptable rate.
    pub max_rate: Option<String>,
    /// Maximum source data age in seconds.
    pub max_source_age_secs: Option<u64>,
    /// Alert cooldown in seconds.
    pub alert_cooldown_secs: Option<u64>,
    /// Whether to record anomalies.
    pub record_anomalies: Option<bool>,
    /// Bootstrap (initial state) safety settings.
    pub bootstrap: Option<RawBootstrapSafetyConfig>,
    /// Provider consensus settings.
    pub consensus: Option<RawConsensusSafetyConfig>,
}

/// Raw bootstrap safety configuration (applies when no previous rate exists).
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawBootstrapSafetyConfig {
    /// Action when a previous rate is missing (`"accept"` or `"quarantine"`).
    pub missing_previous_rate: Option<String>,
}

/// Raw provider consensus safety configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawConsensusSafetyConfig {
    /// Minimum number of successful feeds.
    pub min_successful_feeds: Option<usize>,
    /// Maximum provider spread as a percentage.
    pub max_provider_spread_pct: Option<String>,
    /// Action when consensus fails.
    pub action: Option<String>,
}

/// Raw event system configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawEventsConfig {
    /// Whether the event system is enabled.
    pub enabled: Option<bool>,
    /// Event delivery mode (`"simple"` or `"outbox"`).
    pub mode: Option<String>,
    /// Store backend for events.
    pub store: Option<String>,
    /// Whether to persist events.
    pub record: Option<bool>,
    /// Events table name.
    pub table: Option<String>,
    /// Whether to fail fast on sink errors.
    pub sink_fail_fast: Option<bool>,
    /// Column name overrides.
    pub columns: Option<BTreeMap<String, String>>,
    /// Event routing rules.
    pub routes: Option<Vec<RawEventRouteConfig>>,
    /// Sink definitions.
    pub sinks: Option<BTreeMap<String, RawEventSinkConfig>>,
}

/// Raw event routing rule.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawEventRouteConfig {
    /// Event type pattern to match.
    pub event: String,
    /// Sink names to deliver to.
    pub sinks: Vec<String>,
}

/// Raw event sink configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawEventSinkConfig {
    /// Sink kind (`"log"`, `"telegram"`, `"webhook"`).
    pub kind: String,
    /// Log level for log sinks.
    pub level: Option<String>,
    /// Env var for Telegram bot token.
    pub bot_token_env: Option<String>,
    /// Env var for Telegram chat ID.
    pub chat_id_env: Option<String>,
    /// HTTP method for Telegram/webhook sinks.
    pub method: Option<String>,
    /// Telegram parse mode.
    pub parse_mode: Option<String>,
    /// Whether to disable Telegram link previews.
    pub disable_web_page_preview: Option<bool>,
    /// Message template for Telegram sinks.
    pub message: Option<String>,
    /// Env var for webhook URL.
    pub url_env: Option<String>,
    /// Custom HTTP headers for webhook sinks.
    pub headers: Option<BTreeMap<String, String>>,
    /// Webhook body configuration.
    pub body: Option<RawWebhookBodyConfig>,
}

/// Raw webhook body configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawWebhookBodyConfig {
    /// Body format (`"json"`, `"text"`, etc.).
    pub format: String,
    /// Body template with placeholder substitution.
    pub template: String,
}

/// Raw outbox configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawOutboxConfig {
    /// Whether the outbox is enabled.
    pub enabled: Option<bool>,
    /// Store backend for outbox.
    pub store: Option<String>,
    /// Outbox table name.
    pub table: Option<String>,
    /// Dispatch interval in seconds.
    pub dispatch_interval_secs: Option<u64>,
    /// Maximum delivery retries.
    pub max_retries: Option<u32>,
    /// Retry backoff in seconds.
    pub retry_backoff_secs: Option<u64>,
    /// Request timeout in seconds.
    pub request_timeout_secs: Option<u64>,
    /// Column name overrides.
    pub columns: Option<BTreeMap<String, String>>,
}

/// Raw provider configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawProviderConfig {
    /// Provider kind (`"static"` or `"http_json"`).
    pub kind: String,
    /// HTTP method for HTTP-based providers.
    pub method: Option<String>,
    /// URL template for HTTP-based providers.
    pub url_template: Option<String>,
    /// Authentication settings.
    pub auth: Option<RawProviderAuthConfig>,
    /// JSON path expressions.
    pub paths: Option<RawProviderPathsConfig>,
    /// Timestamp format settings.
    pub formats: Option<RawProviderFormatsConfig>,
}

/// Raw provider authentication configuration.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawProviderAuthConfig {
    /// HTTP header name.
    pub header: String,
    /// Environment variable name holding the header value.
    pub value_env: String,
}

/// Raw provider JSON path expressions.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawProviderPathsConfig {
    /// JSON path to the rate value.
    pub rate: Option<String>,
    /// JSON path to the source timestamp.
    pub source_updated_at: Option<String>,
}

/// Raw provider timestamp format settings.
#[cfg_attr(feature = "config-toml", derive(Deserialize))]
#[cfg_attr(feature = "config-toml", serde(deny_unknown_fields))]
#[derive(Clone, Debug)]
pub struct RawProviderFormatsConfig {
    /// Timestamp format (`"rfc3339"`, `"unix"`, `"unix_ms"`).
    pub source_updated_at: Option<String>,
}
