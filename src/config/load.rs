use crate::config::resolved::ResolvedConfig;
use crate::error::{Error, Result};

#[cfg(feature = "config-toml")]
use crate::config::env::expand_env;
#[cfg(feature = "config-toml")]
use crate::config::raw::{RawConfig, RawEventSinkArrayEntry, RawTransportsConfig};
#[cfg(feature = "config-toml")]
use crate::config::validate::resolve_config;
#[cfg(feature = "config-toml")]
use std::collections::BTreeMap;
#[cfg(feature = "config-toml")]
use toml::Value;

#[cfg(feature = "config-toml")]
/// Load and resolve a configuration from a TOML file path.
///
/// Uses a two-stage parsing approach for universal config compatibility:
/// 1. Parse the whole TOML into a [`toml::Value`] table.
/// 2. Extract only known root sections (version, log, stores, http, chains,
///    assets, transports, oracles). Ignore unrelated namespaces (artur, bria,
///    ladon, pano, meta, runtime, paths, objects, transports.amqp).
/// 3. Validate that the `oracles` table contains no unknown fields.
/// 4. Convert array-format `[[oracles.events.sinks]]` to the map format.
/// 5. Deserialize the filtered content into [`RawConfig`].
/// 6. Run the full validation pipeline via [`resolve_config`].
pub fn load_config(path: impl AsRef<std::path::Path>) -> Result<ResolvedConfig> {
    let text = std::fs::read_to_string(path)?;
    let (raw, transports) = parse_universal_config(&text)?;
    resolve_config(raw, transports)
}

#[cfg(not(feature = "config-toml"))]
/// Stub that returns an error when the `config-toml` feature is disabled.
pub fn load_config(_path: impl AsRef<std::path::Path>) -> Result<ResolvedConfig> {
    Err(Error::Config("config-toml feature is disabled".to_owned()))
}

#[cfg(feature = "config-toml")]
/// Parse a TOML string using the universal config model.
///
/// Returns the extracted [`RawConfig`] and optional [`RawTransportsConfig`].
fn parse_universal_config(text: &str) -> Result<(RawConfig, RawTransportsConfig)> {
    let root: Value = toml::from_str(text)
        .map_err(|err| Error::Config(format!("failed to parse TOML: {err}")))?;

    let Value::Table(mut root_table) = root else {
        return Err(Error::Config("config root must be a TOML table".to_owned()));
    };

    // Known root sections that oracles understands.
    const KNOWN_ROOT_KEYS: &[&str] = &[
        "version", "log", "stores", "http", "chains", "assets", "oracles",
    ];

    // Extract transports before filtering.
    let transports = extract_transports(&root_table)?;

    // Extract the oracles table for unknown-field validation.
    let oracles_val = root_table.get("oracles").cloned();

    // Build a filtered root containing only known sections.
    let mut filtered = toml::map::Map::new();
    for key in KNOWN_ROOT_KEYS {
        if let Some(val) = root_table.remove(*key) {
            filtered.insert((*key).to_owned(), val);
        }
    }

    // Expand every string in the shared and Oracles-owned sections before
    // deserialization and validation. Unrelated package namespaces remain
    // untouched, so their required environment variables are not imposed on
    // the standalone Oracles worker.
    let mut filtered_value = Value::Table(filtered);
    expand_env_values(&mut filtered_value)?;
    let Value::Table(filtered) = filtered_value else {
        return Err(Error::Config("config root must be a TOML table".to_owned()));
    };

    // Validate unknown fields inside the `oracles` table.
    if let Some(ref ov) = oracles_val {
        validate_oracles_unknown_fields(ov)?;
    }

    // Convert array-format event sinks if present.
    let filtered = convert_array_sinks(filtered)?;

    let raw: RawConfig = toml::Value::Table(filtered)
        .try_into()
        .map_err(|err| Error::Config(format!("failed to deserialize config: {err}")))?;

    Ok((raw, transports))
}

#[cfg(feature = "config-toml")]
/// Extract universal transport profiles from the root table.
fn extract_transports(root: &toml::map::Map<String, Value>) -> Result<RawTransportsConfig> {
    let Some(Value::Table(transports)) = root.get("transports") else {
        return Ok(RawTransportsConfig::default());
    };

    let mut http_profiles = BTreeMap::new();
    let mut webhook_profiles = BTreeMap::new();

    if let Some(Value::Table(http_tbl)) = transports.get("http") {
        for (id, val) in http_tbl {
            let mut value = val.clone();
            expand_env_values(&mut value)?;
            let profile = value.try_into().map_err(|err| {
                Error::Config(format!("invalid [transports.http.{id}] profile: {err}"))
            })?;
            http_profiles.insert(id.clone(), profile);
        }
    }

    if let Some(Value::Table(wh_tbl)) = transports.get("webhook") {
        for (id, val) in wh_tbl {
            let mut value = val.clone();
            expand_env_values(&mut value)?;
            let profile = value.try_into().map_err(|err| {
                Error::Config(format!("invalid [transports.webhook.{id}] profile: {err}"))
            })?;
            webhook_profiles.insert(id.clone(), profile);
        }
    }

    Ok(RawTransportsConfig {
        http: http_profiles,
        webhook: webhook_profiles,
    })
}

#[cfg(feature = "config-toml")]
/// Expand environment placeholders in every string nested under a supported
/// universal-config section.
fn expand_env_values(value: &mut Value) -> Result<()> {
    match value {
        Value::String(string) => *string = expand_env(string)?,
        Value::Array(values) => {
            for value in values {
                expand_env_values(value)?;
            }
        }
        Value::Table(values) => {
            for (_, value) in values {
                expand_env_values(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(feature = "config-toml")]
/// Validate that the `oracles` table contains no unknown fields.
///
/// Walks all keys in the oracles table and checks them against the known
/// set of fields for [`RawOraclesConfig`].
fn validate_oracles_unknown_fields(oracles: &Value) -> Result<()> {
    let Value::Table(table) = oracles else {
        return Ok(());
    };

    // Known keys for RawOraclesConfig.
    const KNOWN_ORACLES_KEYS: &[&str] = &[
        "store",
        "quote",
        "refresh_secs",
        "stale_after_secs",
        "max_source_age_secs",
        "max_concurrent_requests",
        "fail_fast",
        "selection",
        "table",
        "safety",
        "events",
        "outbox",
        "providers",
        "asset_ids",
        "assets",
    ];

    for key in table.keys() {
        if !KNOWN_ORACLES_KEYS.contains(&key.as_str()) {
            return Err(Error::Config(format!(
                "unknown field `{key}` in [oracles] section"
            )));
        }
    }

    Ok(())
}

#[cfg(feature = "config-toml")]
/// Convert canonical array-format event sinks `[[oracles.events.sinks]]` to
/// the internal map format.
fn convert_array_sinks(
    mut root: toml::map::Map<String, Value>,
) -> Result<toml::map::Map<String, Value>> {
    // Navigate to oracles.events.sinks
    let Some(Value::Table(oracles)) = root.get_mut("oracles") else {
        return Ok(root);
    };
    let Some(Value::Table(events)) = oracles.get_mut("events") else {
        return Ok(root);
    };
    let Some(sinks_val) = events.get("sinks") else {
        return Ok(root);
    };

    if let Value::Table(_) = sinks_val {
        return Err(Error::Config(
            "[oracles.events.sinks] must use [[oracles.events.sinks]] entries with id and kind"
                .to_owned(),
        ));
    }

    // Convert canonical array entries to the internal map.
    if let Value::Array(arr) = sinks_val {
        let mut entries: Vec<RawEventSinkArrayEntry> = Vec::new();

        for val in arr {
            match val.clone().try_into() {
                Ok(entry) => entries.push(entry),
                Err(_e) => {
                    // If conversion fails, leave sinks as-is; the normal
                    // deserializer will produce a clear error.
                    return Ok(root);
                }
            }
        }

        let mut sinks_map = toml::map::Map::new();
        for entry in entries {
            let id = entry.id.clone();
            let mut table = toml::map::Map::new();

            // Insert basic fields.
            table.insert("kind".to_owned(), Value::String(entry.kind.clone()));

            if let Some(enabled) = entry.enabled {
                table.insert("enabled".to_owned(), Value::Boolean(enabled));
            }
            if let Some(ref level) = entry.level {
                table.insert("level".to_owned(), Value::String(level.clone()));
            }
            if let Some(ref bot_token_env) = entry.bot_token_env {
                table.insert(
                    "bot_token_env".to_owned(),
                    Value::String(bot_token_env.clone()),
                );
            }
            if let Some(ref chat_id_env) = entry.chat_id_env {
                table.insert("chat_id_env".to_owned(), Value::String(chat_id_env.clone()));
            }
            if let Some(ref method) = entry.method {
                table.insert("method".to_owned(), Value::String(method.clone()));
            }
            if let Some(ref parse_mode) = entry.parse_mode {
                table.insert("parse_mode".to_owned(), Value::String(parse_mode.clone()));
            }
            if let Some(disable_web_page_preview) = entry.disable_web_page_preview {
                table.insert(
                    "disable_web_page_preview".to_owned(),
                    Value::Boolean(disable_web_page_preview),
                );
            }
            if let Some(ref message) = entry.message {
                table.insert("message".to_owned(), Value::String(message.clone()));
            }
            if let Some(ref url_env) = entry.url_env {
                table.insert("url_env".to_owned(), Value::String(url_env.clone()));
            }
            if let Some(ref headers) = entry.headers {
                let mut header_map = toml::map::Map::new();
                for (k, v) in headers {
                    header_map.insert(k.clone(), Value::String(v.clone()));
                }
                table.insert("headers".to_owned(), Value::Table(header_map));
            }
            if let Some(ref body) = entry.body {
                let mut body_map = toml::map::Map::new();
                body_map.insert("format".to_owned(), Value::String(body.format.clone()));
                body_map.insert("template".to_owned(), Value::String(body.template.clone()));
                table.insert("body".to_owned(), Value::Table(body_map));
            }
            if let Some(ref transport) = entry.transport {
                table.insert("transport".to_owned(), Value::String(transport.clone()));
            }
            if let Some(ref url) = entry.url {
                table.insert("url".to_owned(), Value::String(url.clone()));
            }
            if let Some(ref token) = entry.token {
                table.insert("token".to_owned(), Value::String(token.clone()));
            }
            if let Some(timeout_secs) = entry.timeout_secs {
                table.insert(
                    "timeout_secs".to_owned(),
                    Value::Integer(timeout_secs as i64),
                );
            }
            if let Some(max_retries) = entry.max_retries {
                table.insert("max_retries".to_owned(), Value::Integer(max_retries as i64));
            }
            if let Some(retry_base_ms) = entry.retry_base_ms {
                table.insert(
                    "retry_base_ms".to_owned(),
                    Value::Integer(retry_base_ms as i64),
                );
            }

            if sinks_map.insert(id.clone(), Value::Table(table)).is_some() {
                return Err(Error::Config(format!(
                    "duplicate sink id `{id}` in [[oracles.events.sinks]]"
                )));
            }
        }

        events.insert("sinks".to_owned(), Value::Table(sinks_map));
    }

    Ok(root)
}

#[cfg(all(feature = "config-toml", test))]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_universal_ignores_unrelated_namespaces() {
        let toml = r#"
version = 1

[log]
level = "info"
format = "json"

[stores.oracles]
driver = "sqlite"
url = "sqlite://test.db"

[http]
user_agent = "test"

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

[oracles.providers.static]
kind = "static"

# Unrelated sections that should be ignored
[ladon]
enabled = true
store = "ladon"

[pano]
enabled = true
store = "pano"

[bria]
enabled = true

[meta]
name = "test"

[runtime]
worker_threads = 0

[paths.some_path]
kind = "file"
path = "/tmp/test"

[objects.local]
driver = "fs"
root = "/tmp"
"#;
        let result = parse_universal_config(toml);
        assert!(result.is_ok(), "unexpected error: {result:?}");
        let (raw, _transports) = result.unwrap();
        assert_eq!(raw.version, 1);
        assert_eq!(raw.oracles.store, "oracles");
    }

    #[test]
    fn rejects_unknown_fields_in_oracles() {
        let toml = r#"
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
unknown_bad_field = "should fail"

[oracles.providers.static]
kind = "static"
"#;
        let result = parse_universal_config(toml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("unknown_bad_field"),
            "expected error about unknown field, got: {err}"
        );
    }

    #[test]
    fn expands_oracles_quote_before_validation() {
        let toml = r#"
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
quote = "${ORACLES_TEST_UNSET_QUOTE_9BF4D6:-USD}"
refresh_secs = 60
stale_after_secs = 120

[oracles.providers.static]
kind = "static"
"#;
        let (raw, transports) = parse_universal_config(toml).expect("valid universal config");
        let resolved = resolve_config(raw, transports).expect("expanded quote must validate");
        assert_eq!(resolved.oracles.quote.as_str(), "USD");
    }

    #[test]
    fn parse_universal_with_transports() {
        let toml = r#"
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

[oracles.providers.static]
kind = "static"

[transports.http.default]
base_url = "https://api.example.com"
timeout_secs = 30

[transports.webhook.ops]
url = "https://hooks.example.com"
method = "POST"
timeout_secs = 10
"#;
        let result = parse_universal_config(toml);
        assert!(result.is_ok(), "unexpected error: {result:?}");
        let (_raw, transports) = result.unwrap();
        assert!(transports.http.contains_key("default"));
        assert!(transports.webhook.contains_key("ops"));
    }

    #[test]
    fn rejects_invalid_oracles_transport_profile()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let toml = r#"
[transports.http.default]
timeout_secs = 30
unknown_field = true
"#;
        let err = parse_universal_config(toml)
            .err()
            .ok_or("unknown fields in an Oracles HTTP transport must fail")?
            .to_string();
        assert!(err.contains("transports.http.default"));
        assert!(err.contains("unknown_field"));
        Ok(())
    }

    #[test]
    fn parse_universal_with_array_event_sinks() {
        let toml = r#"
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

[oracles.providers.static]
kind = "static"

[oracles.events]
enabled = false

[[oracles.events.sinks]]
id = "ops-log"
kind = "log"
level = "warn"
enabled = true
"#;
        let result = parse_universal_config(toml);
        assert!(result.is_ok(), "unexpected error: {result:?}");
        let (raw, _transports) = result.unwrap();
        let events = raw.oracles.events.unwrap();
        let sinks = events.sinks.unwrap();
        assert!(sinks.contains_key("ops-log"));
        let sink = &sinks["ops-log"];
        assert_eq!(sink.kind, "log");
        assert_eq!(sink.level.as_deref(), Some("warn"));
    }

    #[test]
    fn rejects_duplicate_array_event_sink_ids()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let root: Value = toml::from_str(
            r#"
[oracles.events]

[[oracles.events.sinks]]
id = "ops-log"
kind = "log"

[[oracles.events.sinks]]
id = "ops-log"
kind = "log"
"#,
        )?;
        let root = root
            .as_table()
            .cloned()
            .ok_or("TOML root must be a table")?;

        let err = convert_array_sinks(root)
            .err()
            .ok_or("duplicate sink IDs must fail")?
            .to_string();
        assert!(err.contains("duplicate sink id `ops-log`"));
        Ok(())
    }

    #[test]
    fn rejects_legacy_map_event_sink_shape() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let root: Value = toml::from_str(
            r#"
[oracles.events.sinks.ops_log]
kind = "log"
"#,
        )?;
        let root = root
            .as_table()
            .cloned()
            .ok_or("TOML root must be a table")?;

        let err = convert_array_sinks(root)
            .err()
            .ok_or("legacy map event sink shape must fail")?
            .to_string();
        assert!(err.contains("must use [[oracles.events.sinks]]"));
        Ok(())
    }

    #[test]
    fn parse_oracles_assets_feeds() {
        let toml = r#"
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
asset_ids = ["eth"]

[oracles.providers.static]
kind = "static"

[oracles.assets.eth]
enabled = true

[[oracles.assets.eth.feeds]]
enabled = true
provider = "static"
priority = 100
params = { rate = "1.00" }
"#;
        let result = parse_universal_config(toml);
        assert!(result.is_ok(), "unexpected error: {result:?}");
        let (raw, _transports) = result.unwrap();
        assert_eq!(
            raw.oracles.asset_ids.as_deref(),
            Some(&["eth".to_owned()][..])
        );
        let oracle_assets = raw.oracles.assets.unwrap();
        assert!(oracle_assets.contains_key("eth"));
        let eth_asset = &oracle_assets["eth"];
        let feeds = eth_asset.feeds.as_ref().unwrap();
        assert_eq!(feeds.len(), 1);
    }
}
