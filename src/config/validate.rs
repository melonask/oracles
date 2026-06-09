use crate::config::env::expand_env;
use crate::config::raw::*;
use crate::config::resolved::*;
use crate::domain::{AssetId, ChainId, EventAction, ProviderId, Quote, RateAmount};
use crate::error::{Error, Result};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::str::FromStr;
use time::Duration;

/// Resolve a raw configuration into a fully-validated [`ResolvedConfig`].
///
/// This function performs all validation: checks cross-references between
/// assets, chains, providers, and stores; resolves defaults; expands
/// environment variables; and ensures all identifiers are well-formed.
pub fn resolve_config(raw: RawConfig) -> Result<ResolvedConfig> {
    if raw.version != 1 {
        return Err(Error::Config(format!(
            "unsupported config version: {}",
            raw.version
        )));
    }

    let stores = resolve_stores(raw.stores)?;
    let http = resolve_http(raw.http)?;
    let chains = resolve_chains(raw.chains)?;

    if !stores.contains_key(&raw.oracles.store) {
        return Err(Error::Config(format!(
            "oracles.store references unknown store: {}",
            raw.oracles.store
        )));
    }

    let log = resolve_log(raw.log)?;

    // Resolve everything that borrows from raw.oracles first, before partial moves.
    let oracles = resolve_oracles(&raw.oracles)?;
    let safety = resolve_safety(&raw.oracles, &oracles)?;
    // Validate that stale_after_secs is at least refresh_secs, otherwise
    // every rate expires before the next refresh cycle.
    if oracles.stale_after_secs < oracles.refresh_secs {
        return Err(Error::Config(format!(
            "oracles.stale_after_secs ({}) must be >= oracles.refresh_secs ({})",
            oracles.stale_after_secs, oracles.refresh_secs
        )));
    }
    // Now move-owned fields out of raw.oracles.
    let providers = resolve_providers(raw.oracles.providers)?;
    let events = resolve_events(raw.oracles.events, &raw.oracles.outbox)?;
    let outbox = resolve_outbox(raw.oracles.outbox, &events)?;
    let assets = resolve_assets(raw.assets, &chains, &providers)?;

    // Forbid events.record=false + outbox mode — produces orphaned outbox rows.
    if matches!(events.mode, EventMode::Outbox) && !events.record {
        return Err(Error::Config(
            "events.record must be true when events.mode = \"outbox\"".to_owned(),
        ));
    }

    // outbox mode requires record_anomalies for safety events.
    if matches!(events.mode, EventMode::Outbox) && !safety.record_anomalies {
        return Err(Error::Config(
            "events.mode = \"outbox\" requires oracles.safety.record_anomalies = true for safety events".to_owned(),
        ));
    }

    // compare_against = "last_observed" requires event recording.
    if safety.compare_against == CompareAgainst::LastObserved {
        if !events.enabled {
            return Err(Error::Config(
                "safety.compare_against = \"last_observed\" requires events.enabled = true"
                    .to_owned(),
            ));
        }
        if !events.record {
            return Err(Error::Config(
                "safety.compare_against = \"last_observed\" requires events.record = true"
                    .to_owned(),
            ));
        }
    }

    // disable_asset requires durable events.
    if safety.default_action == EventAction::DisableAsset {
        if !events.enabled {
            return Err(Error::Config(
                "safety.default_action = \"disable_asset\" requires events.enabled = true"
                    .to_owned(),
            ));
        }
        if !events.record {
            return Err(Error::Config(
                "safety.default_action = \"disable_asset\" requires events.record = true"
                    .to_owned(),
            ));
        }
        if !safety.record_anomalies {
            return Err(Error::Config(
                "safety.default_action = \"disable_asset\" requires oracles.safety.record_anomalies = true".to_owned(),
            ));
        }
    }
    for asset in &assets {
        if let Some(ref action) = asset.safety_action
            && action == &EventAction::DisableAsset
        {
            if !events.enabled {
                return Err(Error::Config(format!(
                    "asset \"{}\" safety.action = \"disable_asset\" requires events.enabled = true",
                    asset.id.as_str()
                )));
            }
            if !events.record {
                return Err(Error::Config(format!(
                    "asset \"{}\" safety.action = \"disable_asset\" requires events.record = true",
                    asset.id.as_str()
                )));
            }
            if !safety.record_anomalies {
                return Err(Error::Config(format!(
                    "asset \"{}\" safety.action = \"disable_asset\" requires oracles.safety.record_anomalies = true",
                    asset.id.as_str()
                )));
            }
        }
    }

    // table sink requires events.record.
    for (sink_name, sink) in &events.sinks {
        if matches!(sink, ResolvedEventSink::Table) && !events.record {
            return Err(Error::Config(format!(
                "events sink \"{sink_name}\" of kind \"table\" requires events.record = true"
            )));
        }
    }

    // validate store references for events and outbox.
    if events.enabled && !stores.contains_key(&events.store) {
        return Err(Error::Config(format!(
            "events.store references unknown store: {}",
            events.store
        )));
    }
    if outbox.enabled && !stores.contains_key(&outbox.store) {
        return Err(Error::Config(format!(
            "outbox.store references unknown store: {}",
            outbox.store
        )));
    }

    // Path B: validate store routing equality (independent routing not yet implemented).
    if events.enabled && events.store != oracles.store {
        return Err(Error::Config(format!(
            "events.store ({}) must equal oracles.store ({}) — independent store routing is not yet implemented",
            events.store, oracles.store
        )));
    }
    if outbox.enabled && outbox.store != oracles.store {
        return Err(Error::Config(format!(
            "outbox.store ({}) must equal oracles.store ({}) — independent store routing is not yet implemented",
            outbox.store, oracles.store
        )));
    }

    // reject duplicate physical table names when stores route to the same backend.
    if oracles.table.name == events.table {
        return Err(Error::Config(format!(
            "oracles.table.name ({}) and events.table ({}) must not be the same physical table",
            oracles.table.name, events.table
        )));
    }
    if oracles.table.name == outbox.table {
        return Err(Error::Config(format!(
            "oracles.table.name ({}) and outbox.table ({}) must not be the same physical table",
            oracles.table.name, outbox.table
        )));
    }
    if events.table == outbox.table {
        return Err(Error::Config(format!(
            "events.table ({}) and outbox.table ({}) must not be the same physical table",
            events.table, outbox.table
        )));
    }

    // Validate feature availability for configured sinks and stores.
    #[cfg(not(feature = "telegram"))]
    for (sink_name, sink) in &events.sinks {
        if matches!(sink, ResolvedEventSink::Telegram { .. }) {
            return Err(Error::Config(format!(
                "telegram sink \"{sink_name}\" requires the `telegram` feature to be enabled at compile time"
            )));
        }
    }

    #[cfg(not(feature = "webhook"))]
    for (sink_name, sink) in &events.sinks {
        if matches!(sink, ResolvedEventSink::Webhook { .. }) {
            return Err(Error::Config(format!(
                "webhook sink \"{sink_name}\" requires the `webhook` feature to be enabled at compile time"
            )));
        }
    }

    #[cfg(not(feature = "postgres"))]
    for (store_name, store_config) in &stores {
        if store_config.driver == StoreDriver::Postgres {
            return Err(Error::Config(format!(
                "store \"{store_name}\" uses the postgres driver, which requires the `postgres` feature to be enabled at compile time"
            )));
        }
    }

    Ok(ResolvedConfig {
        version: raw.version,
        log,
        stores,
        http,
        chains,
        assets,
        oracles,
        safety,
        events,
        outbox,
        providers,
    })
}

fn resolve_stores(
    raw: BTreeMap<String, RawStoreConfig>,
) -> Result<BTreeMap<String, ResolvedStoreConfig>> {
    let mut stores = BTreeMap::new();

    for (id, store) in raw {
        let driver = match store.driver.as_str() {
            "sqlite" => StoreDriver::Sqlite,
            "postgres" => StoreDriver::Postgres,
            other => {
                return Err(Error::Config(format!(
                    "unknown store driver `{other}` for store `{id}`"
                )));
            }
        };

        let connect_timeout_secs = store.connect_timeout_secs.unwrap_or(10);
        let max_connections = store.max_connections.unwrap_or(1);

        // PostgreSQL only supports max_connections = 1.
        if driver == StoreDriver::Postgres && max_connections != 1 {
            return Err(Error::Config(format!(
                "PostgreSQL store \"{id}\" currently only supports max_connections = 1. \
                 Got: {max_connections}. Connection pooling is not yet implemented."
            )));
        }

        // SQLite only supports max_connections = 1.
        if driver == StoreDriver::Sqlite && max_connections != 1 {
            return Err(Error::Config(format!(
                "SQLite store \"{id}\" only supports max_connections = 1. \
                 Got: {max_connections}. Connection pooling is not supported for SQLite."
            )));
        }

        // validate positive values.
        if connect_timeout_secs == 0 {
            return Err(Error::Config(format!(
                "stores.{id}.connect_timeout_secs must be > 0"
            )));
        }
        if max_connections == 0 {
            return Err(Error::Config(format!(
                "stores.{id}.max_connections must be > 0"
            )));
        }

        stores.insert(
            id,
            ResolvedStoreConfig {
                driver,
                url: expand_env(&store.url)?,
                migrate: store.migrate.unwrap_or(true),
                connect_timeout_secs,
                max_connections,
            },
        );
    }

    Ok(stores)
}

fn resolve_http(raw: Option<RawHttpConfig>) -> Result<ResolvedHttpConfig> {
    let raw = raw.unwrap_or(RawHttpConfig {
        user_agent: None,
        request_timeout_secs: None,
        max_retries: None,
        retry_backoff_ms: None,
    });

    let request_timeout_secs = raw.request_timeout_secs.unwrap_or(15);
    // validate positive timeout.
    if request_timeout_secs == 0 {
        return Err(Error::Config(
            "http.request_timeout_secs must be > 0".to_owned(),
        ));
    }

    Ok(ResolvedHttpConfig {
        user_agent: raw.user_agent.unwrap_or_else(|| "oracles/0.1".to_owned()),
        request_timeout_secs,
        max_retries: raw.max_retries.unwrap_or(3),
        retry_backoff_ms: raw.retry_backoff_ms.unwrap_or(500),
    })
}

fn resolve_chains(
    raw: BTreeMap<String, RawChainConfig>,
) -> Result<BTreeMap<ChainId, ResolvedChain>> {
    let mut chains = BTreeMap::new();

    for (id, chain) in raw {
        let id = ChainId::new(id)?;

        let resolved = ResolvedChain {
            id: id.clone(),
            family: chain.family,
            caip2: chain.caip2,
            native_symbol: chain.native_symbol,
            rpc_urls: chain.rpc_urls.unwrap_or_default(),
            confirmations: chain.confirmations.unwrap_or(0),
        };

        chains.insert(id, resolved);
    }

    Ok(chains)
}

fn resolve_providers(
    raw: BTreeMap<String, RawProviderConfig>,
) -> Result<BTreeMap<ProviderId, ResolvedProvider>> {
    let mut providers = BTreeMap::new();

    for (id, provider) in raw {
        let id = ProviderId::new(id)?;

        let kind = match provider.kind.as_str() {
            "static" => ProviderKind::Static,
            "http_json" => ProviderKind::HttpJson,
            other => {
                return Err(Error::Config(format!(
                    "unknown provider kind `{other}` for provider `{}`",
                    id.as_str()
                )));
            }
        };

        if kind == ProviderKind::HttpJson && provider.url_template.is_none() {
            return Err(Error::Config(format!(
                "http_json provider `{}` requires url_template",
                id.as_str()
            )));
        }

        // validate HTTP method for http_json providers.
        if kind == ProviderKind::HttpJson {
            match provider.method.as_deref().unwrap_or("GET") {
                "GET" | "POST" => {}
                other => {
                    return Err(Error::Config(format!(
                        "http_json provider `{}` has unsupported method \"{other}\"; must be GET or POST",
                        id.as_str()
                    )));
                }
            }
        }

        let auth = provider.auth.map(|auth| ResolvedProviderAuth {
            header: auth.header,
            value_env: auth.value_env,
        });

        let rate_path = provider.paths.as_ref().and_then(|paths| paths.rate.clone());

        let source_updated_at_path = provider
            .paths
            .as_ref()
            .and_then(|paths| paths.source_updated_at.clone());

        let source_updated_at_format = provider
            .formats
            .as_ref()
            .and_then(|formats| formats.source_updated_at.clone());

        // validate provider timestamp format.
        if let Some(ref fmt) = source_updated_at_format {
            match fmt.as_str() {
                "rfc3339" | "unix" | "unix_ms" => {}
                other => {
                    return Err(Error::Config(format!(
                        "provider `{}` has unsupported source_updated_at_format \"{other}\"; must be rfc3339, unix, or unix_ms",
                        id.as_str()
                    )));
                }
            }
        }

        providers.insert(
            id.clone(),
            ResolvedProvider {
                id,
                kind,
                method: provider.method,
                url_template: provider.url_template,
                auth,
                rate_path,
                source_updated_at_path,
                source_updated_at_format,
            },
        );
    }

    Ok(providers)
}

fn resolve_oracles(raw: &RawOraclesConfig) -> Result<ResolvedOraclesConfig> {
    let quote = Quote::new(raw.quote.clone())?;

    let refresh_secs = if raw.refresh_secs < 1 {
        return Err(Error::Config(format!(
            "oracles.refresh_secs must be >= 1, got: {}",
            raw.refresh_secs,
        )));
    } else {
        raw.refresh_secs
    };

    let max_concurrent_requests = raw.max_concurrent_requests.unwrap_or(8);
    if max_concurrent_requests < 1 {
        return Err(Error::Config(format!(
            "oracles.max_concurrent_requests must be >= 1, got: {}",
            max_concurrent_requests
        )));
    }

    let selection = match raw.selection.as_deref().unwrap_or("priority") {
        "priority" => SelectionMode::Priority,
        "all" => SelectionMode::All,
        "median" => SelectionMode::Median,
        other => {
            return Err(Error::Config(format!("unknown oracles.selection: {other}")));
        }
    };

    // Resolve rate column mappings.
    let raw_rate_columns = raw.table.as_ref().and_then(|t| t.columns.clone());
    let rate_columns = resolve_rate_columns(raw_rate_columns)?;

    let table = match raw.table.as_ref() {
        None => ResolvedRateTableConfig {
            name: "oracle_rates".to_owned(),
            write_mode: WriteMode::Upsert,
            columns: rate_columns,
        },
        Some(table) => {
            let write_mode = match table.write_mode.as_deref().unwrap_or("upsert") {
                "upsert" => WriteMode::Upsert,
                "append" => WriteMode::Append,
                other => {
                    return Err(Error::Config(format!(
                        "unknown oracle table write_mode: {other}"
                    )));
                }
            };

            ResolvedRateTableConfig {
                name: table.name.clone(),
                write_mode,
                columns: rate_columns,
            }
        }
    };

    // validate table name as a SQL identifier.
    validate_identifier_config(&table.name, "oracles.table.name")?;

    Ok(ResolvedOraclesConfig {
        store: raw.store.clone(),
        quote,
        refresh_secs,
        stale_after_secs: raw.stale_after_secs,
        max_source_age_secs: raw.max_source_age_secs,
        max_concurrent_requests,
        fail_fast: raw.fail_fast.unwrap_or(false),
        selection,
        table,
    })
}

fn resolve_safety(
    raw: &RawOraclesConfig,
    oracles: &ResolvedOraclesConfig,
) -> Result<ResolvedSafetyConfig> {
    let safety = raw.safety.as_ref();

    let default_action = parse_action(
        safety
            .and_then(|s| s.default_action.as_deref())
            .unwrap_or("quarantine"),
    )?;

    let compare_against = match safety
        .and_then(|s| s.compare_against.as_deref())
        .unwrap_or("last_accepted")
    {
        "last_accepted" => CompareAgainst::LastAccepted,
        "last_observed" => CompareAgainst::LastObserved,
        other => {
            return Err(Error::Config(format!(
                "unknown safety.compare_against: {other}"
            )));
        }
    };

    let max_change_pct = parse_decimal(
        safety
            .and_then(|s| s.max_change_pct.as_deref())
            .unwrap_or("50"),
        "oracles.safety.max_change_pct",
    )?;

    let min_rate = safety
        .and_then(|s| s.min_rate.as_deref())
        .map(RateAmount::parse)
        .transpose()?;

    let max_rate = safety
        .and_then(|s| s.max_rate.as_deref())
        .map(RateAmount::parse)
        .transpose()?;

    let max_source_age_secs = safety
        .and_then(|s| s.max_source_age_secs)
        .or(raw.max_source_age_secs);

    let max_source_age = max_source_age_secs.map(|secs| Duration::seconds(secs as i64));

    let bootstrap = safety.and_then(|s| s.bootstrap.as_ref());

    let bootstrap_action = match bootstrap
        .and_then(|b| b.missing_previous_rate.as_deref())
        .unwrap_or("accept")
    {
        "accept" => BootstrapAction::Accept,
        "quarantine" => BootstrapAction::Quarantine,
        "require_multiple_providers" => BootstrapAction::RequireMultipleProviders,
        other => {
            return Err(Error::Config(format!(
                "unknown safety.bootstrap.missing_previous_rate: {other}"
            )));
        }
    };

    let consensus_raw = safety.and_then(|s| s.consensus.as_ref());

    let consensus = ResolvedConsensusConfig {
        min_successful_feeds: consensus_raw
            .and_then(|c| c.min_successful_feeds)
            .unwrap_or(1),
        max_provider_spread_pct: parse_decimal(
            consensus_raw
                .and_then(|c| c.max_provider_spread_pct.as_deref())
                .unwrap_or("5"),
            "oracles.safety.consensus.max_provider_spread_pct",
        )?,
        action: parse_consensus_action(
            consensus_raw
                .and_then(|c| c.action.as_deref())
                .unwrap_or("quarantine"),
        )?,
    };

    if consensus.min_successful_feeds < 1 {
        return Err(Error::Config(
            "oracles.safety.consensus.min_successful_feeds must be >= 1".to_owned(),
        ));
    }

    if max_change_pct < Decimal::ZERO {
        return Err(Error::Config(
            "oracles.safety.max_change_pct must be non-negative".to_owned(),
        ));
    }

    if consensus.max_provider_spread_pct < Decimal::ZERO {
        return Err(Error::Config(
            "oracles.safety.consensus.max_provider_spread_pct must be non-negative".to_owned(),
        ));
    }

    Ok(ResolvedSafetyConfig {
        enabled: safety.and_then(|s| s.enabled).unwrap_or(true),
        compare_against,
        default_action,
        max_change_pct,
        min_rate,
        max_rate,
        max_source_age,
        stale_after: Duration::seconds(oracles.stale_after_secs as i64),
        alert_cooldown_secs: safety.and_then(|s| s.alert_cooldown_secs).unwrap_or(3600),
        record_anomalies: safety.and_then(|s| s.record_anomalies).unwrap_or(true),
        bootstrap_action,
        consensus,
    })
}

fn resolve_assets(
    raw: BTreeMap<String, RawAssetConfig>,
    chains: &BTreeMap<ChainId, ResolvedChain>,
    providers: &BTreeMap<ProviderId, ResolvedProvider>,
) -> Result<Vec<ResolvedAsset>> {
    let mut assets = Vec::new();

    for (asset_id, asset) in raw {
        let id = AssetId::new(asset_id)?;
        let chain_id = ChainId::new(asset.chain.clone())?;

        let Some(chain) = chains.get(&chain_id) else {
            return Err(Error::Config(format!(
                "asset `{}` references unknown chain `{}`",
                id.as_str(),
                chain_id.as_str()
            )));
        };

        let mut feeds = Vec::new();

        for feed in asset.feeds.unwrap_or_default() {
            let provider = ProviderId::new(feed.provider)?;

            if !providers.contains_key(&provider) {
                return Err(Error::Config(format!(
                    "asset `{}` references unknown provider `{}`",
                    id.as_str(),
                    provider.as_str()
                )));
            }

            feeds.push(ResolvedFeed {
                enabled: feed.enabled.unwrap_or(true),
                provider,
                priority: feed.priority,
                params: feed.params.unwrap_or_default(),
            });
        }

        if feeds.is_empty() && asset.enabled.unwrap_or(true) {
            return Err(Error::Config(format!(
                "asset `{}` must define at least one feed when enabled",
                id.as_str()
            )));
        }

        let enabled = asset.enabled.unwrap_or(true);

        let x402 = asset.x402.map(|x| ResolvedX402Config {
            enabled: x.enabled.unwrap_or(true),
            asset_address: x.asset_address,
            transfer_method: x.transfer_method,
        });

        if asset.decimals > 18 {
            return Err(Error::Config(format!(
                "asset `{}` decimals must be <= 18, got: {}",
                id.as_str(),
                asset.decimals
            )));
        }

        let safety = asset.safety;

        assets.push(ResolvedAsset {
            id,
            enabled,
            chain_id,
            caip2: chain.caip2.clone(),
            symbol: asset.symbol,
            name: asset.name,
            kind: asset.kind,
            contract: asset.contract,
            decimals: asset.decimals,
            x402,
            feeds,
            safety_enabled: safety.as_ref().and_then(|s| s.enabled).unwrap_or(true),
            safety_max_change_pct: safety
                .as_ref()
                .and_then(|s| s.max_change_pct.as_deref())
                .map(|v| parse_decimal(v, "asset.safety.max_change_pct"))
                .transpose()?,
            safety_min_rate: safety
                .as_ref()
                .and_then(|s| s.min_rate.as_deref())
                .map(RateAmount::parse)
                .transpose()?,
            safety_max_rate: safety
                .as_ref()
                .and_then(|s| s.max_rate.as_deref())
                .map(RateAmount::parse)
                .transpose()?,
            safety_action: safety
                .as_ref()
                .and_then(|s| s.action.as_deref())
                .map(parse_action)
                .transpose()?,
        });
    }

    Ok(assets)
}

fn resolve_events(
    raw_events: Option<RawEventsConfig>,
    raw_outbox: &Option<RawOutboxConfig>,
) -> Result<ResolvedEventsConfig> {
    let request_timeout_secs = raw_outbox
        .as_ref()
        .and_then(|o| o.request_timeout_secs)
        .unwrap_or(10);
    let Some(events) = raw_events else {
        let table = "oracle_events".to_owned();
        // validate table name as a SQL identifier.
        validate_identifier_config(&table, "events.table")?;
        return Ok(ResolvedEventsConfig {
            enabled: false,
            mode: EventMode::Simple,
            store: "oracles".to_owned(),
            record: false,
            table,
            sink_fail_fast: false,
            columns: ResolvedEventColumns::defaults(),
            routes: Vec::new(),
            sinks: BTreeMap::new(),
        });
    };

    let mode = match events.mode.as_deref().unwrap_or("simple") {
        "simple" => EventMode::Simple,
        "outbox" => EventMode::Outbox,
        other => return Err(Error::Config(format!("unknown events.mode: {other}"))),
    };

    let mut sinks = BTreeMap::new();

    for (id, sink) in events.sinks.unwrap_or_default() {
        let resolved = match sink.kind.as_str() {
            "log" => {
                let level = sink.level.unwrap_or_else(|| "warn".to_owned());

                // validate log sink levels.
                match level.as_str() {
                    "trace" | "debug" | "info" | "warn" | "error" => {}
                    other => {
                        return Err(Error::Config(format!(
                            "log sink \"{id}\" has invalid level \"{other}\"; must be trace, debug, info, warn, or error"
                        )));
                    }
                }

                ResolvedEventSink::Log { level }
            }
            "telegram" => {
                let method = sink.method.unwrap_or_else(|| "POST".to_owned());

                // validate Telegram method is POST.
                if method != "POST" {
                    return Err(Error::Config(format!(
                        "telegram sink \"{id}\" has unsupported method \"{method}\"; only POST is supported"
                    )));
                }

                ResolvedEventSink::Telegram {
                    bot_token_env: sink.bot_token_env.ok_or_else(|| {
                        Error::Config(format!("telegram sink `{id}` requires bot_token_env"))
                    })?,
                    chat_id_env: sink.chat_id_env.ok_or_else(|| {
                        Error::Config(format!("telegram sink `{id}` requires chat_id_env"))
                    })?,
                    method,
                    parse_mode: sink.parse_mode,
                    disable_web_page_preview: sink.disable_web_page_preview.unwrap_or(true),
                    message: sink.message.unwrap_or_else(|| "{event_type}".to_owned()),
                    timeout_secs: request_timeout_secs,
                }
            }
            "table" => ResolvedEventSink::Table,
            "webhook" => {
                let method = sink.method.unwrap_or_else(|| "POST".to_owned());

                // validate webhook method.
                match method.as_str() {
                    "POST" | "PUT" | "PATCH" => {}
                    other => {
                        return Err(Error::Config(format!(
                            "webhook sink \"{id}\" has unsupported method \"{other}\"; must be POST, PUT, or PATCH"
                        )));
                    }
                }

                let body = sink
                    .body
                    .ok_or_else(|| Error::Config(format!("webhook sink `{id}` requires body")))?;

                // validate webhook body format.
                match body.format.as_str() {
                    "json" | "text" => {}
                    other => {
                        return Err(Error::Config(format!(
                            "webhook sink \"{id}\" has unsupported body format \"{other}\"; must be json or text"
                        )));
                    }
                }

                ResolvedEventSink::Webhook {
                    url_env: sink.url_env.ok_or_else(|| {
                        Error::Config(format!("webhook sink `{id}` requires url_env"))
                    })?,
                    method,
                    headers: sink.headers.unwrap_or_default(),
                    body_format: body.format,
                    body_template: body.template,
                    timeout_secs: request_timeout_secs,
                }
            }
            other => {
                return Err(Error::Config(format!(
                    "unknown event sink kind `{other}` for sink `{id}`"
                )));
            }
        };

        sinks.insert(id, resolved);
    }

    let routes = events
        .routes
        .unwrap_or_default()
        .into_iter()
        .map(|route| {
            // validate event route names.
            if !is_valid_event_type(&route.event) {
                return Err(Error::Config(format!(
                    "unknown event type \"{}\" in route; must be one of: oracle.rate_anomaly, oracle.rate_quarantined, oracle.rate_rejected, oracle.provider_failed, oracle.refresh_failed",
                    route.event
                )));
            }

            for sink in &route.sinks {
                if !sinks.contains_key(sink) {
                    return Err(Error::Config(format!(
                        "event route `{}` references unknown sink `{sink}`",
                        route.event
                    )));
                }
            }

            Ok(ResolvedEventRoute {
                event: route.event,
                sinks: route.sinks,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Resolve event column mappings.
    let event_columns = resolve_event_columns(events.columns)?;

    let table = events.table.unwrap_or_else(|| "oracle_events".to_owned());
    // validate table name as a SQL identifier.
    validate_identifier_config(&table, "events.table")?;

    Ok(ResolvedEventsConfig {
        enabled: events.enabled.unwrap_or(true),
        mode,
        store: events.store.unwrap_or_else(|| "oracles".to_owned()),
        record: events.record.unwrap_or(true),
        table,
        sink_fail_fast: events.sink_fail_fast.unwrap_or(false),
        columns: event_columns,
        routes,
        sinks,
    })
}

fn resolve_outbox(
    raw_outbox: Option<RawOutboxConfig>,
    events: &ResolvedEventsConfig,
) -> Result<ResolvedOutboxConfig> {
    let enabled = raw_outbox
        .as_ref()
        .and_then(|outbox| outbox.enabled)
        .unwrap_or(matches!(events.mode, EventMode::Outbox));

    if matches!(events.mode, EventMode::Outbox) && !enabled {
        return Err(Error::Config(
            "events.mode = \"outbox\" requires outbox.enabled = true".to_owned(),
        ));
    }

    let dispatch_interval_secs = raw_outbox
        .as_ref()
        .and_then(|outbox| outbox.dispatch_interval_secs)
        .unwrap_or(10);
    let max_retries = raw_outbox
        .as_ref()
        .and_then(|outbox| outbox.max_retries)
        .unwrap_or(5);
    let retry_backoff_secs = raw_outbox
        .as_ref()
        .and_then(|outbox| outbox.retry_backoff_secs)
        .unwrap_or(30);
    let request_timeout_secs = raw_outbox
        .as_ref()
        .and_then(|outbox| outbox.request_timeout_secs)
        .unwrap_or(10);

    // validate positive values.
    if dispatch_interval_secs == 0 {
        return Err(Error::Config(
            "outbox.dispatch_interval_secs must be > 0".to_owned(),
        ));
    }
    if max_retries == 0 {
        return Err(Error::Config("outbox.max_retries must be > 0".to_owned()));
    }
    if retry_backoff_secs == 0 {
        return Err(Error::Config(
            "outbox.retry_backoff_secs must be > 0".to_owned(),
        ));
    }
    if request_timeout_secs == 0 {
        return Err(Error::Config(
            "outbox.request_timeout_secs must be > 0".to_owned(),
        ));
    }

    // Resolve outbox column mappings.
    let outbox_columns = resolve_outbox_columns(
        raw_outbox
            .as_ref()
            .and_then(|outbox| outbox.columns.clone()),
    )?;

    let table = raw_outbox
        .as_ref()
        .and_then(|outbox| outbox.table.clone())
        .unwrap_or_else(|| "oracle_outbox".to_owned());
    // validate table name as a SQL identifier.
    validate_identifier_config(&table, "outbox.table")?;

    Ok(ResolvedOutboxConfig {
        enabled,
        store: raw_outbox
            .as_ref()
            .and_then(|outbox| outbox.store.clone())
            .unwrap_or_else(|| events.store.clone()),
        table,
        dispatch_interval_secs,
        max_retries,
        retry_backoff_secs,
        request_timeout_secs,
        columns: outbox_columns,
    })
}

/// Resolve raw logging config into a validated [`ResolvedLogConfig`].
fn resolve_log(raw: Option<RawLogConfig>) -> Result<ResolvedLogConfig> {
    let level = raw
        .as_ref()
        .and_then(|r| r.level.as_deref())
        .unwrap_or("info")
        .to_owned();

    let format = raw
        .as_ref()
        .and_then(|r| r.format.as_deref())
        .unwrap_or("json")
        .to_owned();

    // Validate log level
    match level.as_str() {
        "trace" | "debug" | "info" | "warn" | "error" => {}
        other => {
            return Err(Error::Config(format!(
                "invalid log level: {other}, must be one of: trace, debug, info, warn, error"
            )));
        }
    }

    // Validate log format
    match format.as_str() {
        "json" | "pretty" | "compact" => {}
        other => {
            return Err(Error::Config(format!(
                "invalid log format: {other}, must be one of: json, pretty, compact"
            )));
        }
    }

    Ok(ResolvedLogConfig { level, format })
}

/// Parse a string into an [`EventAction`].
///
/// Recognised values: `"alert"`, `"quarantine"`, `"reject"`, `"disable_asset"`.
pub fn parse_action(input: &str) -> Result<EventAction> {
    match input {
        "alert" => Ok(EventAction::Alert),
        "quarantine" => Ok(EventAction::Quarantine),
        "reject" => Ok(EventAction::Reject),
        "disable_asset" => Ok(EventAction::DisableAsset),
        other => Err(Error::Config(format!("unknown safety action: {other}"))),
    }
}

/// Parse a string into an [`EventAction`] for the consensus context.
///
/// Same as [`parse_action`] but rejects `"disable_asset"` because it is not a
/// valid consensus action.
fn parse_consensus_action(input: &str) -> Result<EventAction> {
    match input {
        "alert" => Ok(EventAction::Alert),
        "quarantine" => Ok(EventAction::Quarantine),
        "reject" => Ok(EventAction::Reject),
        "disable_asset" => Err(Error::Config(
            "consensus action cannot be \"disable_asset\"; must be alert, quarantine, or reject"
                .to_owned(),
        )),
        other => Err(Error::Config(format!("unknown safety action: {other}"))),
    }
}

/// Known event type names for route validation.
const VALID_EVENT_TYPES: &[&str] = &[
    "oracle.rate_anomaly",
    "oracle.rate_quarantined",
    "oracle.rate_rejected",
    "oracle.provider_failed",
    "oracle.refresh_failed",
];

/// Return `true` if `name` is a recognised event type name.
fn is_valid_event_type(name: &str) -> bool {
    VALID_EVENT_TYPES.contains(&name)
}

fn parse_decimal(input: &str, field: &str) -> Result<Decimal> {
    Decimal::from_str(input).map_err(|_| Error::Config(format!("{field} must be a decimal string")))
}

// ---------------------------------------------------------------------------
// Column mapping helpers
// ---------------------------------------------------------------------------

/// SQL reserved words that should be rejected as identifiers.
const SQL_RESERVED_WORDS: &[&str] = &[
    // Common SQL reserved words (SQLite + PostgreSQL intersection)
    "select",
    "insert",
    "update",
    "delete",
    "create",
    "drop",
    "alter",
    "table",
    "index",
    "view",
    "trigger",
    "where",
    "from",
    "join",
    "on",
    "and",
    "or",
    "not",
    "null",
    "is",
    "in",
    "like",
    "between",
    "as",
    "order",
    "group",
    "having",
    "limit",
    "offset",
    "union",
    "except",
    "intersect",
    "into",
    "values",
    "set",
    "primary",
    "key",
    "foreign",
    "references",
    "check",
    "default",
    "constraint",
    "unique",
    "cascade",
    "restrict",
    "if",
    "exists",
    "case",
    "when",
    "then",
    "else",
    "end",
    "begin",
    "commit",
    "rollback",
    "transaction",
    "true",
    "false",
    "unknown",
];

fn validate_identifier_config(value: &str, field: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::Config(format!("{field} must not be empty")));
    }
    let first = value.as_bytes()[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return Err(Error::Config(format!(
            "{field} must start with a letter or underscore, got: {value}"
        )));
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(Error::Config(format!(
            "{field} contains invalid characters, only [A-Za-z0-9_] allowed: {value}"
        )));
    }
    if SQL_RESERVED_WORDS.contains(&value.to_ascii_lowercase().as_str()) {
        return Err(Error::Config(format!(
            "{field} is a SQL reserved word and cannot be used as an identifier: {value}"
        )));
    }
    Ok(())
}

/// Generic column-map resolution helper.
///
/// Takes an optional raw column map from the config, a `Vec` of `(key,
/// &mut default_value)` pairs, and:
///
/// 1. Rejects unknown keys in the raw map.
/// 2. Overlays user-provided values onto the defaults.
/// 3. Validates every resolved column name via [`validate_identifier_config`].
/// 4. Rejects duplicate resolved column names across different logical columns.
fn resolve_column_map(
    raw_columns: Option<BTreeMap<String, String>>,
    mut fields: Vec<(&str, &mut String)>,
) -> Result<()> {
    let known_keys: Vec<&str> = fields.iter().map(|(k, _)| *k).collect();

    if let Some(raw) = raw_columns {
        for key in raw.keys() {
            if !known_keys.contains(&key.as_str()) {
                return Err(Error::Config(format!(
                    "unknown column key \"{key}\" in columns map"
                )));
            }
        }

        for (field_name, target) in fields.iter_mut() {
            if let Some(value) = raw.get(*field_name) {
                **target = value.clone();
            }
        }
    }

    for (field_name, value) in fields.iter() {
        validate_identifier_config(value, &format!("columns.{field_name}"))?;
    }

    // Detect duplicate resolved column names.
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for (field_name, value) in fields.iter() {
        if let Some(prev_field) = seen.get(value.as_str()) {
            return Err(Error::Config(format!(
                "duplicate column name \"{value}\" in columns map ({prev_field} and {field_name} map to same column)"
            )));
        }
        seen.insert(value.as_str(), field_name);
    }

    Ok(())
}

/// Resolve rate table column mappings.
fn resolve_rate_columns(
    raw_columns: Option<BTreeMap<String, String>>,
) -> Result<ResolvedRateColumns> {
    let mut columns = ResolvedRateColumns::defaults();
    resolve_column_map(
        raw_columns,
        vec![
            ("id", &mut columns.id),
            ("asset_id", &mut columns.asset_id),
            ("chain_id", &mut columns.chain_id),
            ("caip2", &mut columns.caip2),
            ("symbol", &mut columns.symbol),
            ("quote", &mut columns.quote),
            ("provider", &mut columns.provider),
            ("rate", &mut columns.rate),
            ("source_updated_at", &mut columns.source_updated_at),
            ("observed_at", &mut columns.observed_at),
            ("expires_at", &mut columns.expires_at),
        ],
    )?;
    Ok(columns)
}

/// Resolve event table column mappings.
fn resolve_event_columns(
    raw_columns: Option<BTreeMap<String, String>>,
) -> Result<ResolvedEventColumns> {
    let mut columns = ResolvedEventColumns::defaults();
    resolve_column_map(
        raw_columns,
        vec![
            ("id", &mut columns.id),
            ("event_type", &mut columns.event_type),
            ("asset_id", &mut columns.asset_id),
            ("chain_id", &mut columns.chain_id),
            ("symbol", &mut columns.symbol),
            ("quote", &mut columns.quote),
            ("provider", &mut columns.provider),
            ("previous_rate", &mut columns.previous_rate),
            ("candidate_rate", &mut columns.candidate_rate),
            ("change_pct", &mut columns.change_pct),
            ("action", &mut columns.action),
            ("reason", &mut columns.reason),
            ("source_updated_at", &mut columns.source_updated_at),
            ("observed_at", &mut columns.observed_at),
        ],
    )?;
    Ok(columns)
}

/// Resolve outbox table column mappings.
fn resolve_outbox_columns(
    raw_columns: Option<BTreeMap<String, String>>,
) -> Result<ResolvedOutboxColumns> {
    let mut columns = ResolvedOutboxColumns::defaults();
    resolve_column_map(
        raw_columns,
        vec![
            ("id", &mut columns.id),
            ("event_id", &mut columns.event_id),
            ("sink", &mut columns.sink),
            ("payload", &mut columns.payload),
            ("status", &mut columns.status),
            ("attempts", &mut columns.attempts),
            ("next_attempt_at", &mut columns.next_attempt_at),
            ("delivered_at", &mut columns.delivered_at),
            ("last_error", &mut columns.last_error),
        ],
    )?;
    Ok(columns)
}
