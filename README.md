# Oracles

<img align="right" src="https://raw.githubusercontent.com/melonask/oracles/refs/heads/main/logo.svg" alt="Oracles is a Rust service and library for fetching cryptocurrency exchange rates from configured providers, validating candidate rates, and storing accepted rates in SQLite or PostgreSQL" width="200" />

> **Oracles** — Where Prophecy Meets Crypto

`oracles` is a Rust service and library for fetching cryptocurrency exchange rates from configured providers, validating candidate rates, and storing accepted rates in SQLite or PostgreSQL.

It is designed for payment, balance, billing, x402, and automation systems that need fresh fiat-denominated rates without making the oracle process itself the source of truth.

## Core ideas

- The worker is stateless.
- Durable storage is the source of truth for accepted rates, events, and outbox deliveries.
- Rates are stored as decimal strings, not floating-point values.
- Provider timestamps and local observation timestamps are stored separately.
- Suspicious rates can be alerted, quarantined, rejected, or used to disable an asset.
- Events are separate from safety decisions.
- Notification delivery can use a durable outbox.
- Secrets belong in environment variables, not in `Config.toml`.

## Package layout

```text
src/
  cli/        CLI parsing and command execution
  config/     raw config structs, environment expansion, validation, resolved config
  domain/     identifiers, rates, events, decisions
  engine/     oracle refresh engine and scheduler
  events/     event templates, sinks, outbox dispatcher
  provider/   static and HTTP JSON providers
  safety/     rate safety engine
  store/      RateStore/OutboxStore traits, SQLite, PostgreSQL
  x402.rs     x402 pricing helpers
```

## Feature flags

Default features: `cli`, `config-toml`, `http-json`, `sqlite`.

| Feature | Default | Description |
|---|---:|---|
| `cli` | yes | Builds the `oracles` binary and CLI entrypoint. |
| `config-toml` | yes | Loads TOML config files with `serde` + `toml`. |
| `http-json` | yes | Enables HTTP JSON providers using `ureq` and `serde_json`. |
| `sqlite` | yes | Enables the SQLite store backend. |
| `postgres` | no | Enables the PostgreSQL store backend. |
| `pg` | no | Alias for `postgres`. Enables the PostgreSQL store backend. |
| `postgres-tls` | no | Enables PostgreSQL TLS support. Implies `postgres`. |
| `telegram` | no | Enables the Telegram event sink. Implies `http-json` for the shared HTTP client dependency. |
| `webhook` | no | Enables the webhook event sink. Implies `http-json` for the shared HTTP client dependency. |
| `outbox` | no | Compatibility no-op. Outbox types are implemented by the store/event system. |
| `full` | no | Enables all optional features: CLI, TOML config, HTTP JSON, SQLite, PostgreSQL, TLS, Telegram, webhook, and outbox. |

## Installation

Default SQLite-focused install:

```bash
cargo install oracles
```

Full install with PostgreSQL, TLS, Telegram, and webhook support:

```bash
cargo install oracles --features full
```

For local development:

```bash
git clone https://github.com/melonask/oracles.git
cd oracles
cargo test --all-targets --all-features
```

## Quick start

Copy the example config:

```bash
cp Config.example.toml Config.toml
```

The provided `Config.example.toml` is intentionally verbose and includes Telegram and webhook sinks. To use it unchanged, build/run with `--features full`. With default features, remove or comment out the Telegram/webhook sink definitions and routes before running `--check`.

Validate config:

```bash
oracles --config Config.toml --check
```

Fetch once and exit:

```bash
oracles --config Config.toml --once
```

Run continuously:

```bash
oracles --config Config.toml
```

Override only the runtime log level:

```bash
oracles --config Config.toml --log-level debug
```

## CLI reference

```text
oracles [OPTIONS]

Options:
  --config <path>      Path to config file (default: Config.toml)
  --check              Validate config and exit
  --once               Fetch rates once and exit
  --log-level <level>  Override log level: trace, debug, info, warn, error
  -h, --help           Show help
```

## Docker

```bash
docker run --rm \
  -v "$PWD/Config.toml:/etc/oracles/Config.toml:ro" \
  -v "$PWD/data:/data" \
  ghcr.io/melonask/oracles:latest \
  --config /etc/oracles/Config.toml
```

Build locally:

```bash
docker build -t oracles:local .
```

Run with a mounted config and SQLite data directory:

```bash
docker run --rm \
  -v "$PWD/Config.toml:/etc/oracles/Config.toml:ro" \
  -v "$PWD/data:/data" \
  oracles:local \
  --config /etc/oracles/Config.toml
```

Run with common secrets:

```bash
docker run --rm \
  -e COINGECKO_API_KEY="$COINGECKO_API_KEY" \
  -e TELEGRAM_BOT_TOKEN="$TELEGRAM_BOT_TOKEN" \
  -e TELEGRAM_CHAT_ID="$TELEGRAM_CHAT_ID" \
  -e ORACLES_OPS_WEBHOOK_URL="$ORACLES_OPS_WEBHOOK_URL" \
  -e ORACLES_OPS_WEBHOOK_TOKEN="$ORACLES_OPS_WEBHOOK_TOKEN" \
  -e ORACLES_DATABASE_URL="$ORACLES_DATABASE_URL" \
  -v "$PWD/Config.toml:/etc/oracles/Config.toml:ro" \
  -v "$PWD/data:/data" \
  oracles:local \
  --config /etc/oracles/Config.toml
```

For reproducible Docker/CI builds with `--locked`, keep `Cargo.lock` committed.

## Stateless runtime model

`oracles` should not rely on process memory for correctness.

A normal refresh cycle is:

```text
fetch provider candidate
read previous accepted/observed rate from store
run safety checks
write accepted rate or audit event
write outbox rows when configured
deliver or dispatch notifications
```

A restart should not invalidate safety decisions because previous rates and relevant events are read from durable storage.

## Configuration overview

The root config sections are:

```toml
version = 1

[log]
[stores.<id>]
[http]
[transports.http.<id>]     # optional reusable HTTP client profiles
[transports.webhook.<id>]  # optional reusable webhook profiles
[chains.<id>]
[assets.<id>]              # shared asset identity
[oracles]
[oracles.table]
[oracles.safety]
[oracles.events]
[oracles.outbox]
[oracles.providers.<id>]
[oracles.assets.<id>]      # oracle-specific feed configuration
```

### Universal config model

`oracles` supports loading a merged universal `Config.toml` that may contain
sections for other packages (`[ladon]`, `[pano]`, `[bria]`, `[meta]`,
`[runtime]`, `[paths]`, `[objects]`, `transports.amqp`). These unrelated
namespaces are silently ignored.

Unknown fields inside `[oracles]` are **rejected** with clear error messages.
All shared sections are fully validated.

### Transport profiles

Providers can reference reusable HTTP transport profiles from
`[transports.http.<id>]` via the `transport` field:

```toml
[transports.http.default]
timeout_secs = 30
max_retries = 3

[oracles.providers.coingecko_coin]
kind = "http_json"
transport = "default"
url_template = "https://api.coingecko.com/api/v3/..."
```

Event sinks can reference reusable webhook transport profiles from
`[transports.webhook.<id>]` via the `transport` field:

```toml
[transports.webhook.ops]
url = "${OPS_WEBHOOK_URL:-}"
method = "POST"
timeout_secs = 10

[[oracles.events.sinks]]
id = "ops-webhook"
type = "webhook"
transport = "ops"
```

Reference resolution:
- `transport = "default"` in a provider resolves `[transports.http.default]`.
- `transport = "ops"` in a webhook sink resolves `[transports.webhook.ops]`.
- Unknown transport references fail with actionable errors.
- Package-local values may override shared profile values.

### Oracle-specific assets

Feeds can be configured under `[oracles.assets.<id>]` instead of on the shared
`[[assets.<id>.feeds]]`. This keeps oracle-specific feed logic package-local
while the shared `[assets.<id>]` provides identity metadata:

```toml
[oracles.assets.eth]
enabled = true

[[oracles.assets.eth.feeds]]
enabled = true
provider = "coingecko_coin"
priority = 100
params = { coin_id = "ethereum" }
```

When `[oracles.assets.<id>]` is present, its feeds are used instead of
shared `[[assets.<id>.feeds]]`. The `asset_ids` field in `[oracles]` can
restrict to specific shared assets:

```toml
[oracles]
asset_ids = ["eth", "usdc_base", "sol"]
```

Unknown config fields are rejected by the raw TOML deserializer. Table and column names are validated as unquoted SQL identifiers: they must start with a letter or underscore and contain only ASCII letters, digits, and underscores. Common SQL reserved words are rejected.

Environment expansion supports:

```text
${VAR_NAME}
${VAR_NAME:-default_value}
```

A missing `${VAR_NAME}` without a default is an error.

## Logging

```toml
[log]
level = "info"
format = "json"
```

Allowed levels: `trace`, `debug`, `info`, `warn`, `error`.

Allowed formats:

| Format | Use case |
|---|---|
| `json` | Production containers and log aggregation. |
| `pretty` | Human-readable local logs with timestamps. |
| `compact` | Short local logs. |

## Stores

Stores define durable database backends.

SQLite:

```toml
[stores.oracles]
driver = "sqlite"
url = "sqlite://data/oracles.db"
migrate = true
connect_timeout_secs = 10
max_connections = 1
```

PostgreSQL:

```toml
[stores.oracles]
driver = "postgres"
url = "${ORACLES_DATABASE_URL}"
migrate = true
connect_timeout_secs = 10
max_connections = 1
```

Supported drivers:

| Driver | Feature | URL examples |
|---|---|---|
| `sqlite` | `sqlite` | `sqlite://data/oracles.db`, `sqlite://:memory:`, `sqlite::memory:` |
| `postgres` | `postgres` | `postgres://user:pass@host:5432/db` |

Current limitation: both SQLite and PostgreSQL stores require `max_connections = 1`. Connection pooling is not implemented.

## HTTP defaults

```toml
[http]
user_agent = "oracles/0.1"
request_timeout_secs = 15
max_retries = 3
retry_backoff_ms = 500
```

These settings are used by HTTP JSON providers and by HTTP-based sinks through their resolved timeout settings.

`max_retries` is the number of retries after the first request attempt. The HTTP provider applies exponential backoff and caps the sleep at 30 seconds.

## Chains

Chains provide shared metadata for assets.

```toml
[chains.base]
family = "evm"
caip2 = "eip155:8453"
native_symbol = "ETH"
rpc_urls = ["https://mainnet.base.org"]
confirmations = 12
```

The implementation stores chain metadata and copies CAIP-2 information into rate records. It does not currently perform on-chain RPC reads.

## Assets

Assets define what rates should be fetched.

Native asset:

```toml
[assets.eth]
enabled = true
chain = "eth"
symbol = "ETH"
name = "Ether"
kind = "native"
decimals = 18
```

Token asset:

```toml
[assets.usdc_base]
enabled = true
chain = "base"
symbol = "USDC"
name = "USD Coin on Base"
kind = "erc20"
contract = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
decimals = 6
```

Asset IDs must be lowercase ASCII with digits, underscores, or hyphens. Recommended examples:

```text
eth
sol
usdc_base
usdc_eth
weth_base
```

Avoid using only ticker symbols as asset IDs because the same ticker can exist on many chains.

Config validation rejects `decimals > 18`.

## x402 asset metadata

x402 metadata is optional per asset.

```toml
[assets.eth.x402]
enabled = true
asset_address = "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"
transfer_method = "permit2"
```

The `x402` module provides helpers:

- `convert_fiat_to_asset(fiat_amount, rate, decimals)`
- `x402_price(asset, rate, usd_amount)`
- `format_rate(rate, decimals)`
- `has_x402(asset)`

`convert_fiat_to_asset` returns an amount in the asset’s smallest unit by flooring fractional base units.

## Feeds

Feeds connect assets to providers.

```toml
[[assets.eth.feeds]]
enabled = true
provider = "diadata"
priority = 100
params = { blockchain = "Ethereum", address = "0x0000000000000000000000000000000000000000" }

[[assets.eth.feeds]]
enabled = true
provider = "coingecko_coin"
priority = 50
params = { coin_id = "ethereum" }
```

Higher `priority` values are tried first in `selection = "priority"` mode. Feed `params` are available to provider templates.

Enabled assets must have at least one feed.

## Providers

Providers define reusable fetching/parsing behavior. Assets attach to providers through feeds.

### Static provider

```toml
[oracles.providers.static]
kind = "static"

[[assets.usdc_base.feeds]]
enabled = true
provider = "static"
priority = 100
params = { rate = "1.00000000" }
```

The static provider requires `params.rate` on the feed.

### HTTP JSON provider

```toml
[oracles.providers.diadata]
kind = "http_json"
method = "GET"
url_template = "https://api.diadata.org/v1/assetQuotation/{blockchain}/{address}"

[oracles.providers.diadata.paths]
rate = "Price"
source_updated_at = "Time"

[oracles.providers.diadata.formats]
source_updated_at = "rfc3339"
```

Supported HTTP provider methods: `GET`, `POST`.

Provider authentication uses one header value read from an environment variable:

```toml
[oracles.providers.coingecko_coin.auth]
header = "x-cg-demo-api-key"
value_env = "COINGECKO_API_KEY"
```

### Provider template variables

`url_template` and JSON paths can use `{placeholder}` syntax. Values are URL-encoded in URLs.

Built-in variables:

| Variable | Meaning |
|---|---|
| `{asset_id}` | Internal asset ID, such as `eth`. |
| `{chain_id}` | Internal chain ID. |
| `{caip2}` | CAIP-2 chain ID. |
| `{symbol}` | Asset symbol. |
| `{symbol_lower}` | Lowercase symbol. |
| `{quote}` | Quote currency, such as `USD`. |
| `{quote_lower}` | Lowercase quote, such as `usd`. |
| `{contract}` | Asset contract address, when configured. |
| `{contract_lower}` | Lowercase contract address. |
| `{param}` | Any feed param key. |
| `{param_lower}` | Lowercase form of any feed param value. |

For example, a feed param `coin_id = "ethereum"` creates `{coin_id}` and `{coin_id_lower}`.

### JSON paths

HTTP JSON paths are dot-separated object paths, for example:

```toml
rate = "ethereum.usd"
source_updated_at = "ethereum.last_updated_at"
```

Current JSON path limitations:

- No array indexing.
- No JSONPath filters.
- No escaping for keys that themselves contain dots.

### Timestamp formats

Supported `source_updated_at` formats:

| Format | Expected value |
|---|---|
| `rfc3339` | String such as `2026-06-09T12:00:00Z`. |
| `unix` | Unix seconds as number or string. |
| `unix_ms` | Unix milliseconds as number or string. |

If `source_updated_at` is not configured or not found, the candidate is still usable unless safety rules require otherwise.

## Oracle engine settings

```toml
[oracles]
store = "oracles"
quote = "USD"
refresh_secs = 180
stale_after_secs = 300
max_source_age_secs = 900
max_concurrent_requests = 8
fail_fast = false
selection = "priority"
```

| Option | Default/required | Meaning |
|---|---:|---|
| `store` | required | Store ID from `[stores]`. |
| `quote` | required | Uppercase quote currency. |
| `refresh_secs` | required | Continuous refresh interval. Must be at least 1. |
| `stale_after_secs` | required | Accepted rate validity window. Must be `>= refresh_secs`. |
| `max_source_age_secs` | optional | Fallback source timestamp age limit. |
| `max_concurrent_requests` | 8 | Max concurrent provider fetches in all/median modes. |
| `fail_fast` | false | Stop a refresh cycle after first failed asset. |
| `selection` | `priority` | `priority`, `all`, or `median`. |

`expires_at` is derived:

```text
expires_at = observed_at + stale_after_secs
```

## Selection modes

| Mode | Behavior |
|---|---|
| `priority` | Try enabled feeds in descending priority order and use the first successful candidate. |
| `all` | Fetch all enabled feeds and independently evaluate/write each candidate that passes safety. |
| `median` | Fetch all enabled feeds, run consensus checks, then evaluate the median candidate. |

In `all` and `median` modes, provider fetches can run concurrently when `max_concurrent_requests > 1`.

## Rate table configuration

```toml
[oracles.table]
name = "oracle_rates"
write_mode = "upsert"
```

Supported write modes:

| Mode | Behavior |
|---|---|
| `upsert` | Keep only the latest accepted row per `(asset_id, quote, provider)`. |
| `append` | Insert every accepted observation as a new row. |

Column names can be overridden:

```toml
[oracles.table.columns]
id = "id"
asset_id = "asset_id"
chain_id = "chain_id"
caip2 = "caip2"
symbol = "symbol"
quote = "quote"
provider = "provider"
rate = "rate"
source_updated_at = "source_updated_at"
observed_at = "observed_at"
expires_at = "expires_at"
```

Unknown column keys and duplicate physical column names are rejected.

## Safety checks

```toml
[oracles.safety]
enabled = true
compare_against = "last_accepted"
default_action = "quarantine"
max_change_pct = "50"
min_rate = "0.00000001"
max_source_age_secs = 900
alert_cooldown_secs = 3600
record_anomalies = true
```

Safety checks apply in this order:

1. Source timestamp age.
2. Minimum/maximum rate bounds.
3. Percent change from the previous rate.
4. Bootstrap behavior when no previous rate exists.

### Safety actions

| Action | Effect |
|---|---|
| `alert` | Accept the rate and emit an event. |
| `quarantine` | Record/route an event but do not publish the candidate as an active accepted rate. |
| `reject` | Reject the candidate and record/route an event. |
| `disable_asset` | Record/route an event and skip future refreshes for the asset while the latest relevant event remains `disable_asset`. |

### Baseline selection

```toml
compare_against = "last_accepted"
```

Allowed values:

| Value | Meaning |
|---|---|
| `last_accepted` | Compare to the latest accepted rate in the rates table. |
| `last_observed` | Compare to the latest observed candidate or accepted rate. Requires events to be enabled and recorded. |

### Bootstrap behavior

```toml
[oracles.safety.bootstrap]
missing_previous_rate = "accept"
```

Allowed values:

| Value | Meaning |
|---|---|
| `accept` | Accept the first rate for an asset. |
| `quarantine` | Quarantine when no previous rate exists. |
| `require_multiple_providers` | Require multiple successful providers before accepting the initial rate. |

### Consensus behavior

```toml
[oracles.safety.consensus]
min_successful_feeds = 2
max_provider_spread_pct = "3"
action = "quarantine"
```

Consensus action supports `alert`, `quarantine`, and `reject`. `disable_asset` is intentionally rejected for consensus failures.

## Asset-specific safety overrides

```toml
[assets.eth.safety]
enabled = true
max_change_pct = "50"
min_rate = "10"
max_rate = "100000"
action = "quarantine"
```

Asset-level safety settings override global safety settings for that asset.

Stablecoin example:

```toml
[assets.usdc_base.safety]
enabled = true
max_change_pct = "5"
min_rate = "0.90"
max_rate = "1.10"
action = "quarantine"
```

## Events

Events provide an audit trail and notification input.

```toml
[oracles.events]
enabled = true
mode = "outbox"
store = "oracles"
record = true
table = "oracle_events"
sink_fail_fast = false
```

Supported event modes:

| Mode | Behavior |
|---|---|
| `simple` | Write the event, then deliver matching sinks immediately. |
| `outbox` | Write the event and pending deliveries transactionally, then dispatch later. |

Current limitation: `events.store` must equal `oracles.store` when events are enabled. Independent event-store routing is declared in config but not implemented.

Event types:

```text
oracle.rate_anomaly
oracle.rate_quarantined
oracle.rate_rejected
oracle.provider_failed
oracle.refresh_failed
```

Event reasons:

```text
max_change_exceeded
provider_spread_exceeded
source_timestamp_too_old
rate_below_min
rate_above_max
missing_previous_rate
provider_error
parse_error
```

Event table column overrides:

```toml
[oracles.events.columns]
id = "id"
event_type = "event_type"
asset_id = "asset_id"
chain_id = "chain_id"
symbol = "symbol"
quote = "quote"
provider = "provider"
previous_rate = "previous_rate"
candidate_rate = "candidate_rate"
change_pct = "change_pct"
action = "action"
reason = "reason"
source_updated_at = "source_updated_at"
observed_at = "observed_at"
```

## Event routes

Routes map event types to sink names.

```toml
[[oracles.events.routes]]
event = "oracle.rate_anomaly"
sinks = ["ops_log", "ops_telegram", "ops_webhook"]

[[oracles.events.routes]]
event = "oracle.rate_rejected"
sinks = ["ops_log"]
```

Routes are validated: unknown event names and unknown sink names are rejected.

## Event sinks

### Log sink

```toml
[oracles.events.sinks.ops_log]
kind = "log"
level = "warn"
```

Allowed levels: `trace`, `debug`, `info`, `warn`, `error`.

### Table sink

```toml
[oracles.events.sinks.audit_table]
kind = "table"
```

The table sink is a no-op delivery sink because the event is already written to the events table. It requires `events.record = true`.

### Telegram sink

Requires the `telegram` feature.

```toml
[oracles.events.sinks.ops_telegram]
kind = "telegram"
bot_token_env = "TELEGRAM_BOT_TOKEN"
chat_id_env = "TELEGRAM_CHAT_ID"
method = "POST"
parse_mode = "Markdown"
disable_web_page_preview = true
message = """
*Oracle event*

Type: `{event_type}`
Asset: `{asset_id}`
Provider: `{provider}`
Candidate: `{candidate_rate} {quote}`
Action: `{action}`
Reason: `{reason}`
Observed: `{observed_at}`
"""
```

Only `POST` is supported.

### Webhook sink

Requires the `webhook` feature.

```toml
[oracles.events.sinks.ops_webhook]
kind = "webhook"
url_env = "ORACLES_OPS_WEBHOOK_URL"
method = "POST"

[oracles.events.sinks.ops_webhook.headers]
content-type = "application/json"
authorization = "Bearer ${ORACLES_OPS_WEBHOOK_TOKEN}"

[oracles.events.sinks.ops_webhook.body]
format = "json"
template = """
{
  "event_type": "{event_type}",
  "asset_id": "{asset_id}",
  "quote": "{quote}",
  "provider": "{provider}",
  "candidate_rate": "{candidate_rate}",
  "action": "{action}",
  "reason": "{reason}",
  "observed_at": "{observed_at}"
}
"""
```

Current runtime support: use `method = "POST"`. The validator may accept other methods in the current code, but the delivered sink implementation returns an error for non-POST methods.

### Event template placeholders

Sinks can use these event placeholders:

| Placeholder | Meaning |
|---|---|
| `{event_type}` | Event type string. |
| `{asset_id}` | Internal asset ID. |
| `{chain_id}` | Chain ID, when present. |
| `{symbol}` | Asset symbol. |
| `{quote}` | Quote currency. |
| `{provider}` | Provider ID. |
| `{previous_rate}` | Previous accepted rate, when present. |
| `{candidate_rate}` | Candidate rate, when present. |
| `{change_pct}` | Percent change, when computed. |
| `{action}` | Safety/event action. |
| `{reason}` | Event reason. |
| `{source_updated_at}` | Provider timestamp, when present. |
| `{observed_at}` | Local event timestamp. |

For webhook bodies with `format = "json"`, substituted values are JSON-escaped.

## Outbox

The outbox pattern is recommended for production notification reliability.

```toml
[oracles.outbox]
enabled = true
store = "oracles"
table = "oracle_outbox"
dispatch_interval_secs = 10
max_retries = 5
retry_backoff_secs = 30
request_timeout_secs = 10
```

In outbox mode, the engine writes the decision event and pending sink delivery rows in the same decision transaction. A dispatcher later sends deliveries and marks them `delivered`, `pending`, or `dead`.

Current limitation: `outbox.store` must equal `oracles.store` when outbox is enabled. Independent outbox-store routing is declared in config but not implemented.

Outbox column overrides:

```toml
[oracles.outbox.columns]
id = "id"
event_id = "event_id"
sink = "sink"
payload = "payload"
status = "status"
attempts = "attempts"
next_attempt_at = "next_attempt_at"
delivered_at = "delivered_at"
last_error = "last_error"
```

## Database schema

When `migrate = true`, the store creates the needed tables and indexes automatically. Migration SQL is also provided under `migrations/sqlite` and `migrations/postgres` for external migration tools.

### Rates table

Default SQLite shape:

```sql
CREATE TABLE IF NOT EXISTS oracle_rates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id TEXT NOT NULL,
    chain_id TEXT NOT NULL,
    caip2 TEXT NOT NULL,
    symbol TEXT NOT NULL,
    quote TEXT NOT NULL,
    provider TEXT NOT NULL,
    rate TEXT NOT NULL,
    source_updated_at TEXT,
    observed_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS oracle_rates_asset_quote_idx ON oracle_rates (asset_id, quote);
CREATE INDEX IF NOT EXISTS oracle_rates_expires_at_idx ON oracle_rates (expires_at);
```

Default PostgreSQL shape uses `BIGSERIAL` for `id` and `TIMESTAMPTZ` for timestamp columns.

For PostgreSQL `write_mode = "upsert"`, a unique index on `(asset_id, quote, provider)` is required. The in-code migrator creates it automatically in upsert mode. In append mode, do not create that unique index.

If an existing PostgreSQL database used upsert mode and you switch it to append mode, drop the old unique index first:

```sql
DROP INDEX IF EXISTS oracle_rates_asset_quote_provider_uniq;
```

### Events table

```sql
CREATE TABLE IF NOT EXISTS oracle_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    chain_id TEXT,
    symbol TEXT NOT NULL,
    quote TEXT NOT NULL,
    provider TEXT NOT NULL,
    previous_rate TEXT,
    candidate_rate TEXT,
    change_pct TEXT,
    action TEXT NOT NULL,
    reason TEXT NOT NULL,
    source_updated_at TEXT,
    observed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS oracle_events_asset_quote_idx ON oracle_events (asset_id, quote);
CREATE INDEX IF NOT EXISTS oracle_events_observed_at_idx ON oracle_events (observed_at);
```

### Outbox table

```sql
CREATE TABLE IF NOT EXISTS oracle_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id INTEGER,
    sink TEXT NOT NULL,
    payload TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,
    delivered_at TEXT,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS oracle_outbox_status_next_idx ON oracle_outbox (status, next_attempt_at);
```

## Reading rates from applications

Consumers should read only non-expired accepted rates.

```sql
SELECT rate, provider, observed_at, expires_at
FROM oracle_rates
WHERE asset_id = 'eth'
  AND quote = 'USD'
  AND expires_at > CURRENT_TIMESTAMP
ORDER BY observed_at DESC, id DESC
LIMIT 1;
```

If no row is returned, treat the rate as unavailable or stale.

## Library usage

### Load config and run once with SQLite

```rust
use oracles::Result;
use oracles::config::load_config;
use oracles::engine::Oracle;
use oracles::provider::build_providers;
use oracles::store::sqlite::SqliteRateStore;

fn main() -> Result<()> {
    let config = load_config("Config.toml")?;
    let store = SqliteRateStore::open(&config)?;
    let providers = build_providers(&config)?;

    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once()?;

    eprintln!(
        "attempted={}, succeeded={}, failed={}",
        summary.attempted,
        summary.succeeded,
        summary.failed
    );

    Ok(())
}
```

### Run continuously

```rust
use oracles::engine::scheduler;

// after constructing `oracle`
scheduler::run_loop(&mut oracle);
```

`engine::scheduler::request_shutdown()` is available for embedding applications that install their own signal handlers.

### Dispatch outbox manually

```rust
let summary = oracle.dispatch_outbox(50)?;
eprintln!("delivered={}, failed={}", summary.delivered, summary.failed);
```

### Public API map

Common entry points:

- `config::load_config(path)`
- `config::validate::resolve_config(raw)`
- `provider::build_providers(config)`
- `store::sqlite::SqliteRateStore::open(config)`
- `store::postgres::PostgresRateStore::open(config)`
- `engine::Oracle::new(config, store, providers)`
- `Oracle::run_once()`
- `Oracle::dispatch_outbox(limit)`
- `engine::scheduler::run_loop(&mut oracle)`
- `events::dispatcher::OutboxDispatcher`
- `x402::{convert_fiat_to_asset, x402_price, format_rate, has_x402}`

Core traits:

- `provider::Provider`
- `store::RateStore`
- `store::OutboxStore`
- `events::sinks::EventSink`

## Testing

```bash
# Default SQLite/config/HTTP JSON coverage
cargo test --all-targets

# All optional providers, stores, and notification integrations
cargo test --all-targets --all-features
```

Oracles has no external-service e2e harness in this repository. The integration
suite covers the end-to-end in-process flow from config parsing through provider
selection, safety validation, SQLite storage, event rendering, and outbox logic.

## License

MIT
