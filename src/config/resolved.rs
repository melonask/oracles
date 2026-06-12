use crate::domain::{AssetId, ChainId, EventAction, EventType, ProviderId, Quote, RateAmount};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use time::Duration;

/// Resolved logging configuration.
#[derive(Clone, Debug)]
pub struct ResolvedLogConfig {
    /// The log level (`"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`).
    pub level: String,
    /// The log format (`"json"`, `"pretty"`, `"compact"`).
    pub format: String,
}

/// A fully-validated, ready-to-use oracle configuration.
///
/// Produced by [`crate::config::validate::resolve_config`] from raw TOML
/// or programmatic input. All cross-references have been checked and all
/// defaults have been applied.
#[derive(Clone, Debug)]
pub struct ResolvedConfig {
    /// Config schema version (currently must be `1`).
    pub version: u32,
    /// Logging configuration.
    pub log: ResolvedLogConfig,
    /// Named store backends (SQLite or Postgres).
    pub stores: BTreeMap<String, ResolvedStoreConfig>,
    /// HTTP client settings for providers.
    pub http: ResolvedHttpConfig,
    /// Known blockchain networks.
    pub chains: BTreeMap<ChainId, ResolvedChain>,
    /// Assets whose rates should be tracked.
    pub assets: Vec<ResolvedAsset>,
    /// Oracle engine settings (refresh interval, quote currency, etc.).
    pub oracles: ResolvedOraclesConfig,
    /// Global safety engine settings.
    pub safety: ResolvedSafetyConfig,
    /// Event recording and routing configuration.
    pub events: ResolvedEventsConfig,
    /// Outbox (reliable delivery) configuration.
    pub outbox: ResolvedOutboxConfig,
    /// Provider definitions keyed by provider ID.
    pub providers: BTreeMap<ProviderId, ResolvedProvider>,
}

impl ResolvedConfig {
    /// Return only the assets that are currently enabled.
    pub fn enabled_assets(&self) -> Vec<ResolvedAsset> {
        self.assets
            .iter()
            .filter(|asset| asset.enabled)
            .cloned()
            .collect()
    }

    /// Look up a provider by its ID.
    pub fn provider(&self, id: &ProviderId) -> Option<&ResolvedProvider> {
        self.providers.get(id)
    }
}

/// HTTP client configuration shared across all providers.
#[derive(Clone, Debug)]
pub struct ResolvedHttpConfig {
    /// The `User-Agent` header sent with outgoing requests.
    pub user_agent: String,
    /// Request timeout in seconds.
    pub request_timeout_secs: u64,
    /// Maximum number of retry attempts per request.
    pub max_retries: u32,
    /// Backoff delay between retries, in milliseconds.
    pub retry_backoff_ms: u64,
}

/// Configuration for a single named store backend.
#[derive(Clone, Debug)]
pub struct ResolvedStoreConfig {
    /// The store driver (SQLite or Postgres).
    pub driver: StoreDriver,
    /// Connection URL (e.g., `"sqlite://path/to/db.sqlite"`).
    pub url: String,
    /// Whether to run migrations on open.
    pub migrate: bool,
    /// Connection timeout in seconds.
    pub connect_timeout_secs: u64,
    /// Maximum number of connections in the pool.
    pub max_connections: u32,
}

/// Supported store backends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreDriver {
    /// SQLite (file or in-memory).
    Sqlite,
    /// PostgreSQL.
    Postgres,
}

/// A known blockchain network.
#[derive(Clone, Debug)]
pub struct ResolvedChain {
    /// The chain identifier (e.g., `"eth"`, `"polygon"`).
    pub id: ChainId,
    /// The chain family (e.g., `"evm"`).
    pub family: String,
    /// The CAIP-2 identifier (e.g., `"eip155:1"`).
    pub caip2: String,
    /// The native currency symbol (e.g., `"ETH"`).
    pub native_symbol: Option<String>,
    /// RPC endpoint URLs for this chain.
    pub rpc_urls: Vec<String>,
    /// Required block confirmations for finality.
    pub confirmations: u32,
}

/// A tracked asset with its feeds and safety overrides.
#[derive(Clone, Debug)]
pub struct ResolvedAsset {
    /// The asset identifier (e.g., `"eth"`).
    pub id: AssetId,
    /// Whether this asset is actively tracked.
    pub enabled: bool,
    /// The chain this asset lives on.
    pub chain_id: ChainId,
    /// The CAIP-2 chain identifier for this asset.
    pub caip2: String,
    /// The asset symbol (e.g., `"ETH"`).
    pub symbol: String,
    /// Human-readable name (e.g., `"Ether"`).
    pub name: Option<String>,
    /// The asset kind (e.g., `"native"`, `"erc20"`).
    pub kind: String,
    /// The contract address (for token assets).
    pub contract: Option<String>,
    /// Number of decimals for the asset.
    pub decimals: u8,
    /// Optional X402 (HTTP payment) configuration.
    pub x402: Option<ResolvedX402Config>,
    /// The provider feeds that supply rates for this asset.
    pub feeds: Vec<ResolvedFeed>,

    /// Whether safety checks are enabled for this asset.
    pub safety_enabled: bool,
    /// Maximum allowed percent change from the previous rate (overrides global).
    pub safety_max_change_pct: Option<Decimal>,
    /// Minimum acceptable rate (overrides global).
    pub safety_min_rate: Option<RateAmount>,
    /// Maximum acceptable rate (overrides global).
    pub safety_max_rate: Option<RateAmount>,
    /// Safety action for this asset (overrides global default).
    pub safety_action: Option<EventAction>,
}

impl ResolvedAsset {
    /// Return only the feeds that are currently enabled.
    pub fn enabled_feeds(&self) -> Vec<&ResolvedFeed> {
        self.feeds.iter().filter(|feed| feed.enabled).collect()
    }
}

/// X402 (HTTP 402 Payment Required) configuration for an asset.
#[derive(Clone, Debug)]
pub struct ResolvedX402Config {
    /// Whether X402 is enabled for this asset.
    pub enabled: bool,
    /// The asset's contract address on its chain.
    pub asset_address: String,
    /// The transfer method (e.g., `"erc20_transfer"`).
    pub transfer_method: String,
}

/// A single provider feed for an asset.
#[derive(Clone, Debug)]
pub struct ResolvedFeed {
    /// Whether this feed is currently enabled.
    pub enabled: bool,
    /// The provider that serves this feed.
    pub provider: ProviderId,
    /// Priority (higher values are tried first).
    pub priority: i32,
    /// Provider-specific parameters (e.g., coin IDs, API keys).
    pub params: BTreeMap<String, String>,
}

/// Oracle engine settings.
#[derive(Clone, Debug)]
pub struct ResolvedOraclesConfig {
    /// The store backend to use for rate persistence.
    pub store: String,
    /// The quote currency for all rates (e.g., `"USD"`).
    pub quote: Quote,
    /// How often to refresh rates, in seconds.
    pub refresh_secs: u64,
    /// How long before a rate is considered stale, in seconds.
    pub stale_after_secs: u64,
    /// Maximum allowed age of source data, in seconds.
    pub max_source_age_secs: Option<u64>,
    /// Maximum number of concurrent provider requests.
    pub max_concurrent_requests: usize,
    /// If true, abort on the first failed asset refresh.
    pub fail_fast: bool,
    /// How to select a rate when multiple feeds are available.
    pub selection: SelectionMode,
    /// Rate table configuration.
    pub table: ResolvedRateTableConfig,
}

/// Strategy for selecting a rate from multiple provider feeds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    /// Use the highest-priority feed that returns successfully.
    Priority,
    /// Fetch from all feeds (for recording/audit).
    All,
    /// Use the median value from all successful feeds.
    Median,
}

/// Resolved column name mappings for the rates table.
///
/// Each field holds the physical column name to use in SQL statements.
/// Defaults match the documented `Config.example.toml` column names.
#[derive(Clone, Debug)]
pub struct ResolvedRateColumns {
    /// Primary key / row ID column.
    pub id: String,
    /// Asset identifier column.
    pub asset_id: String,
    /// Chain identifier column.
    pub chain_id: String,
    /// CAIP-2 chain identifier column.
    pub caip2: String,
    /// Asset symbol column.
    pub symbol: String,
    /// Quote currency column.
    pub quote: String,
    /// Provider identifier column.
    pub provider: String,
    /// Accepted rate value column.
    pub rate: String,
    /// Provider-reported source timestamp column.
    pub source_updated_at: String,
    /// Local observation timestamp column.
    pub observed_at: String,
    /// Computed expiry timestamp column.
    pub expires_at: String,
}

impl ResolvedRateColumns {
    /// Return a [`ResolvedRateColumns`] with every field set to its
    /// documented default column name.
    pub fn defaults() -> Self {
        Self {
            id: "id".to_owned(),
            asset_id: "asset_id".to_owned(),
            chain_id: "chain_id".to_owned(),
            caip2: "caip2".to_owned(),
            symbol: "symbol".to_owned(),
            quote: "quote".to_owned(),
            provider: "provider".to_owned(),
            rate: "rate".to_owned(),
            source_updated_at: "source_updated_at".to_owned(),
            observed_at: "observed_at".to_owned(),
            expires_at: "expires_at".to_owned(),
        }
    }
}

/// Resolved column name mappings for the events table.
///
/// Each field holds the physical column name to use in SQL statements.
/// Defaults match the documented `Config.example.toml` column names.
#[derive(Clone, Debug)]
pub struct ResolvedEventColumns {
    /// Primary key / row ID column.
    pub id: String,
    /// Event type string column (e.g. `"oracle.rate_anomaly"`).
    pub event_type: String,
    /// Asset identifier column.
    pub asset_id: String,
    /// Chain identifier column.
    pub chain_id: String,
    /// Asset symbol column.
    pub symbol: String,
    /// Quote currency column.
    pub quote: String,
    /// Provider identifier column.
    pub provider: String,
    /// Previous accepted rate column.
    pub previous_rate: String,
    /// Candidate rate being evaluated column.
    pub candidate_rate: String,
    /// Percentage change column.
    pub change_pct: String,
    /// Safety action taken column.
    pub action: String,
    /// Reason code column.
    pub reason: String,
    /// Provider-reported source timestamp column.
    pub source_updated_at: String,
    /// Local observation timestamp column.
    pub observed_at: String,
}

impl ResolvedEventColumns {
    /// Return a [`ResolvedEventColumns`] with every field set to its
    /// documented default column name.
    pub fn defaults() -> Self {
        Self {
            id: "id".to_owned(),
            event_type: "event_type".to_owned(),
            asset_id: "asset_id".to_owned(),
            chain_id: "chain_id".to_owned(),
            symbol: "symbol".to_owned(),
            quote: "quote".to_owned(),
            provider: "provider".to_owned(),
            previous_rate: "previous_rate".to_owned(),
            candidate_rate: "candidate_rate".to_owned(),
            change_pct: "change_pct".to_owned(),
            action: "action".to_owned(),
            reason: "reason".to_owned(),
            source_updated_at: "source_updated_at".to_owned(),
            observed_at: "observed_at".to_owned(),
        }
    }
}

/// Resolved column name mappings for the outbox table.
///
/// Each field holds the physical column name to use in SQL statements.
/// Defaults match the documented `Config.example.toml` column names.
#[derive(Clone, Debug)]
pub struct ResolvedOutboxColumns {
    /// Primary key / row ID column.
    pub id: String,
    /// Foreign key to the events table column.
    pub event_id: String,
    /// Sink identifier column.
    pub sink: String,
    /// Delivery payload column.
    pub payload: String,
    /// Delivery status column (`"pending"`, `"delivered"`, `"dead"`).
    pub status: String,
    /// Delivery attempt count column.
    pub attempts: String,
    /// Next scheduled attempt timestamp column.
    pub next_attempt_at: String,
    /// Successful delivery timestamp column.
    pub delivered_at: String,
    /// Last delivery error message column.
    pub last_error: String,
}

impl ResolvedOutboxColumns {
    /// Return a [`ResolvedOutboxColumns`] with every field set to its
    /// documented default column name.
    pub fn defaults() -> Self {
        Self {
            id: "id".to_owned(),
            event_id: "event_id".to_owned(),
            sink: "sink".to_owned(),
            payload: "payload".to_owned(),
            status: "status".to_owned(),
            attempts: "attempts".to_owned(),
            next_attempt_at: "next_attempt_at".to_owned(),
            delivered_at: "delivered_at".to_owned(),
            last_error: "last_error".to_owned(),
        }
    }
}

/// Configuration for the rates database table.
#[derive(Clone, Debug)]
pub struct ResolvedRateTableConfig {
    /// The table name for storing accepted rates.
    pub name: String,
    /// How to write rates (upsert or append).
    pub write_mode: WriteMode,
    /// Resolved column name mappings.
    pub columns: ResolvedRateColumns,
}

/// Write strategy for the rates table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// Upsert: insert or update on conflict.
    Upsert,
    /// Append: always insert a new row.
    Append,
}

/// Global safety engine configuration.
#[derive(Clone, Debug)]
pub struct ResolvedSafetyConfig {
    /// Whether the safety engine is enabled.
    pub enabled: bool,
    /// Which rate to compare new candidates against.
    pub compare_against: CompareAgainst,
    /// The default action when a safety rule is triggered.
    pub default_action: EventAction,
    /// Global maximum allowed percent change from the previous rate.
    pub max_change_pct: Decimal,
    /// Global minimum acceptable rate.
    pub min_rate: Option<RateAmount>,
    /// Global maximum acceptable rate.
    pub max_rate: Option<RateAmount>,
    /// Maximum allowed age of source data.
    pub max_source_age: Option<Duration>,
    /// How long before an accepted rate is considered stale.
    pub stale_after: Duration,
    /// Cooldown period between repeated alerts for the same asset, in seconds.
    pub alert_cooldown_secs: u64,
    /// Whether to record anomaly events in the store.
    pub record_anomalies: bool,
    /// What to do when no previous accepted rate exists (bootstrap mode).
    pub bootstrap_action: BootstrapAction,
    /// Provider consensus settings.
    pub consensus: ResolvedConsensusConfig,
}

/// Which rate to use as the baseline for percent-change checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareAgainst {
    /// Compare against the last accepted (persisted) rate.
    LastAccepted,
    /// Compare against the last observed (fetched) rate.
    LastObserved,
}

/// What to do when no previous accepted rate exists for an asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootstrapAction {
    /// Accept the first rate without comparison.
    Accept,
    /// Quarantine the first rate until reviewed.
    Quarantine,
    /// Require multiple providers to agree before accepting.
    RequireMultipleProviders,
}

/// Provider consensus configuration for the safety engine.
#[derive(Clone, Debug)]
pub struct ResolvedConsensusConfig {
    /// Minimum number of successful provider feeds required.
    pub min_successful_feeds: usize,
    /// Maximum allowed spread between providers (as a percentage).
    pub max_provider_spread_pct: Decimal,
    /// Action to take when consensus is not reached.
    pub action: EventAction,
}

/// Event recording and routing configuration.
#[derive(Clone, Debug)]
pub struct ResolvedEventsConfig {
    /// Whether the event system is enabled.
    pub enabled: bool,
    /// Delivery mode (simple or outbox).
    pub mode: EventMode,
    /// The store backend for event persistence.
    pub store: String,
    /// Whether to record events in the store.
    pub record: bool,
    /// The database table for events.
    pub table: String,
    /// If true, abort on the first sink delivery failure.
    pub sink_fail_fast: bool,
    /// Resolved column name mappings.
    pub columns: ResolvedEventColumns,
    /// Routing rules mapping event types to sinks.
    pub routes: Vec<ResolvedEventRoute>,
    /// Configured event sinks keyed by name.
    pub sinks: BTreeMap<String, ResolvedEventSink>,
}

impl ResolvedEventsConfig {
    /// Return the sink names that should receive events of the given type.
    pub fn sinks_for(&self, event_type: &EventType) -> Vec<&str> {
        let event_name = event_type.as_str();

        self.routes
            .iter()
            .filter(|route| route.event == event_name)
            .flat_map(|route| route.sinks.iter().map(String::as_str))
            .collect()
    }

    /// Render a payload for a specific sink from an oracle event.
    ///
    /// Looks up the sink configuration and calls its
    /// [`ResolvedEventSink::render_payload`] method.
    pub fn render_payload(
        &self,
        sink: &str,
        event: &crate::domain::OracleEvent,
    ) -> crate::error::Result<String> {
        let Some(sink_config) = self.sinks.get(sink) else {
            return Err(crate::error::Error::Config("unknown sink".to_owned()));
        };

        sink_config.render_payload(event)
    }
}

/// How events are delivered to sinks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventMode {
    /// Deliver events immediately during oracle refresh.
    Simple,
    /// Queue events in an outbox for reliable delivery.
    Outbox,
}

/// A routing rule mapping an event type to one or more sinks.
#[derive(Clone, Debug)]
pub struct ResolvedEventRoute {
    /// The event type name (e.g., `"oracle.rate_anomaly"`).
    pub event: String,
    /// The sink names that should receive this event type.
    pub sinks: Vec<String>,
}

/// A configured event sink.
///
/// Each variant holds the parameters needed to deliver an event payload.
#[derive(Clone, Debug)]
pub enum ResolvedEventSink {
    /// Write events to stderr with a given log level.
    Log {
        /// The log level (`"error"`, `"warn"`, `"info"`, `"debug"`).
        level: String,
    },
    /// Send events via the Telegram Bot API.
    Telegram {
        /// Environment variable name holding the bot token.
        bot_token_env: String,
        /// Environment variable name holding the chat ID.
        chat_id_env: String,
        /// HTTP method (almost always `"POST"`).
        method: String,
        /// Optional Telegram parse mode (`"HTML"`, `"MarkdownV2"`, etc.).
        parse_mode: Option<String>,
        /// Whether to disable link previews in the Telegram message.
        disable_web_page_preview: bool,
        /// Message template with `{placeholder}` substitution.
        message: String,
        /// HTTP request timeout in seconds.
        timeout_secs: u64,
    },
    /// Send events to an HTTP webhook endpoint.
    Webhook {
        /// Environment variable name holding the webhook URL.
        url_env: String,
        /// HTTP method (almost always `"POST"`).
        method: String,
        /// Custom HTTP headers to include.
        headers: BTreeMap<String, String>,
        /// Body content format (e.g., `"json"`, `"text"`).
        body_format: String,
        /// Body template with `{placeholder}` substitution.
        body_template: String,
        /// HTTP request timeout in seconds.
        timeout_secs: u64,
    },
    /// Write events to the configured event table (no-op for delivery since
    /// events are already recorded by the engine).
    Table,
}

impl ResolvedEventSink {
    /// Render the delivery payload for this sink from an oracle event.
    ///
    /// For `Log` sinks, this renders a debug representation. For `Telegram`
    /// and `Webhook` sinks, it renders the configured template with event
    /// field substitution.
    pub fn render_payload(
        &self,
        event: &crate::domain::OracleEvent,
    ) -> crate::error::Result<String> {
        match self {
            Self::Log { .. } => Ok(format!("{event:?}")),
            Self::Table => Ok(String::new()),
            Self::Telegram { message, .. } => {
                crate::events::template::render_event_template(message, event)
            }
            Self::Webhook {
                body_format,
                body_template,
                ..
            } => match body_format.as_str() {
                "json" => crate::events::template::render_event_template_json(body_template, event),
                _ => crate::events::template::render_event_template(body_template, event),
            },
        }
    }
}

/// Outbox (reliable delivery) configuration.
#[derive(Clone, Debug)]
pub struct ResolvedOutboxConfig {
    /// Whether the outbox is enabled.
    pub enabled: bool,
    /// The store backend for outbox persistence.
    pub store: String,
    /// The database table for outbox entries.
    pub table: String,
    /// How often to attempt dispatching pending deliveries, in seconds.
    pub dispatch_interval_secs: u64,
    /// Maximum number of delivery retries before marking dead.
    pub max_retries: u32,
    /// Backoff delay between retry attempts, in seconds.
    pub retry_backoff_secs: u64,
    /// Request timeout for outbox delivery attempts, in seconds.
    pub request_timeout_secs: u64,
    /// Resolved column name mappings.
    pub columns: ResolvedOutboxColumns,
}

/// A configured rate provider.
#[derive(Clone, Debug)]
pub struct ResolvedProvider {
    /// The provider identifier.
    pub id: ProviderId,
    /// The provider kind (static, HTTP JSON, etc.).
    pub kind: ProviderKind,
    /// HTTP method for HTTP-based providers.
    pub method: Option<String>,
    /// URL template with `{placeholder}` substitution for HTTP-based providers.
    pub url_template: Option<String>,
    /// Optional transport profile reference from [transports.http].
    pub transport: Option<String>,
    /// Authentication configuration.
    pub auth: Option<ResolvedProviderAuth>,
    /// JSON path expression to extract the rate value from the response.
    pub rate_path: Option<String>,
    /// JSON path expression to extract the source timestamp from the response.
    pub source_updated_at_path: Option<String>,
    /// Timestamp format (`"rfc3339"`, `"unix"`, or `"unix_ms"`).
    pub source_updated_at_format: Option<String>,
}

/// Supported provider kinds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    /// A static rate defined in the configuration.
    Static,
    /// An HTTP JSON API provider.
    HttpJson,
}

/// Authentication configuration for an HTTP-based provider.
#[derive(Clone, Debug)]
pub struct ResolvedProviderAuth {
    /// The HTTP header name (e.g., `"Authorization"`).
    pub header: String,
    /// Environment variable name holding the header value.
    pub value_env: String,
}
