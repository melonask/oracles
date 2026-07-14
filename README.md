# Oracles

<img align="right" src="https://raw.githubusercontent.com/melonask/oracles/refs/heads/main/logo.svg" alt="Oracles logo" width="200" />

> **Oracles — Where Prophecy Meets Crypto**

`oracles` is a stateless Rust rate worker and library. It fetches configured cryptocurrency/fiat rates, applies safety policy, and durably records accepted rates, audit events, and notification deliveries in SQLite or PostgreSQL. It is not an HTTP gateway, price authority, x402 enforcer, or chain-RPC client.

[Documentation](https://melonask.github.io/oracles/) · [Getting started](https://melonask.github.io/oracles/guide/getting-started) · [Configuration](https://melonask.github.io/oracles/guide/configuration) · [Repository](https://github.com/melonask/oracles)

## At a glance

| Area | Contract |
|---|---|
| Authority | Durable storage—not memory, logs, provider responses, or notifications—is the source of truth. |
| Rates | Decimal strings; provider `source_updated_at` and local `observed_at` are distinct; `expires_at` is derived. |
| Selection | Static and HTTP JSON providers feed `priority`, `all`, or `median` selection. |
| Safety | Decisions can alert, quarantine, reject, or durably disable an asset; events are audit/notification input, not safety itself. |
| Storage | SQLite and feature-gated PostgreSQL are supported; the process performs no on-chain reads. |
| Delivery | The optional durable outbox is at-least-once; receivers must be idempotent. |

## Requirements, installation, features, and containers

### Requirements

- Rust **1.97** or later to build this release (edition 2024).
- A SQLite database (default feature) or PostgreSQL database (`postgres` feature).
- Network access only for configured HTTP providers and HTTP-based sinks.

### Install

```bash
# Default CLI, TOML config, HTTP JSON, and SQLite support
cargo install oracles

# All optional stores and notification sinks
cargo install oracles --features full
```

For a checkout:

```bash
git clone https://github.com/melonask/oracles.git
cd oracles
cargo test --all-targets --all-features
```

### Features

| Feature | Default | Effect |
|---|---:|---|
| `cli` | yes | Builds the `oracles` binary. |
| `config-toml` | yes | TOML loading and validation. |
| `http-json` | yes | HTTP JSON providers. |
| `sqlite` | yes | SQLite store. |
| `postgres` | no | PostgreSQL store. |
| `postgres-tls` | no | PostgreSQL TLS; implies `postgres`. |
| `telegram` | no | Telegram sink; implies `http-json`. |
| `webhook` | no | Webhook sink; implies `http-json`. |
| `full` | no | All of the above optional capabilities. |

### Container

The official image is built with `full`. Mount configuration read-only and persist the SQLite parent directory when using SQLite:

```bash
docker run --rm \
  -v "$PWD/Config.toml:/etc/oracles/Config.toml:ro" \
  -v "$PWD/data:/data" \
  ghcr.io/melonask/oracles:latest \
  --config /etc/oracles/Config.toml
```

Pass secrets as environment variables, never as TOML literals:

```bash
docker run --rm \
  -e COINGECKO_API_KEY \
  -e TELEGRAM_BOT_TOKEN -e TELEGRAM_CHAT_ID \
  -e ORACLES_OPS_WEBHOOK_URL -e ORACLES_OPS_WEBHOOK_TOKEN \
  -e ORACLES_DATABASE_URL \
  -v "$PWD/Config.toml:/etc/oracles/Config.toml:ro" \
  -v "$PWD/data:/data" \
  ghcr.io/melonask/oracles:latest --config /etc/oracles/Config.toml
```

Build a local image with `docker build -t oracles:local .`. Keep `Cargo.lock` committed for reproducible `--locked` Docker or CI builds.

## Quick start

```bash
cp Config.example.toml Config.toml

# The example config includes Telegram/webhook examples: use a full build,
# or remove/comment those sinks and routes for default features.
oracles --config Config.toml check

# Mutates the configured store: fetch, evaluate, persist, then exit.
oracles --config Config.toml --once

# Start the continuous worker.
oracles --config Config.toml
```

Use a static feed and an isolated SQLite database for a first mutating test. `check` validates configuration but neither opens nor migrates a database and does not fetch providers.

## CLI reference

```text
oracles [OPTIONS] [COMMAND]
oracles ping

Options:
  --config <path>      Path to config (default: $ORACLES_CONFIG, then Config.toml)
  --once               Fetch rates once and exit
  --log-level <level>  Override log level: error, warn, info, debug
  -h, --help           Show help

Commands:
  check                Validate config and exit
  ping                 Print 'pong' and exit
```

The help text above is the built-in output. Its `--log-level` parenthetical omits `trace`, but `trace` is accepted by `LogLevel`, `[log].level`, and `--log-level trace`.

| Operation | Command | Effect | Does **not** prove |
|---|---|---|---|
| Help | `oracles --help` | Prints usage. | Configuration or runtime health. |
| Liveness | `oracles ping` | Writes `pong`; does not load config or open a store. | Features, credentials, DB, providers, or rates. |
| Validation | `oracles --config /absolute/Config.toml check` | Loads, expands, resolves, validates configuration and initializes logging. | Store connectivity/migrations, provider reachability, or sink delivery. |
| One pass | `oracles --config /absolute/Config.toml --once` | Opens/may migrate the store, refreshes enabled assets, then dispatches at most 50 due outbox rows. | Every asset succeeded or every due delivery was drained. |
| Loop | `oracles --config /absolute/Config.toml` | Immediately refreshes and, if enabled, dispatches up to 50 due rows; repeats on configured intervals. | That every cycle or delivery succeeds. |

`--config` wins over `ORACLES_CONFIG`; an unset or empty variable falls back to `Config.toml`. If modes are combined, `ping` wins, then `check`, then `--once`; do not combine modes. `--log-level` overrides only the resolved runtime level.

### Output and exit behavior

Logs are emitted to stderr in the configured `json`, `pretty`, or `compact` format. `ping` writes `pong` to stdout. `--help` exits 0. Any returned configuration, environment, provider, store, safety, template, JSON-path, or CLI error prints `error: …` to stderr and exits 1.

When the configured effective log level permits `info`, a successful `check` writes this message in the configured log format:

```text
Config is valid.
```

When the configured effective log level permits `info`, an ordinary successful one-shot pass writes this summary shape:

```text
Refresh complete: <attempted> attempted, <succeeded> succeeded, <failed> failed
```

Under the same logging condition, if outbox dispatch attempted work, it additionally writes:

```text
outbox: <attempted> attempted, <delivered> delivered, <failed> failed, <dead> dead
```

With `fail_fast = false`, failed assets are counted and the one-shot command can still exit 0; inspect `failed`, failure events, fresh rows, and outbox state. With `fail_fast = true`, the first refresh error is returned and exits 1. A skipped asset with a durable `disable_asset` event is counted as succeeded, so success alone is not proof that a new rate was written. In loop mode refresh/outbox errors are logged and the loop continues. Shutdown is cooperative after the current operation; it does not drain all outbox rows.

## Configuration reference

Start from [`Config.example.toml`](Config.example.toml). `version = 1` is required. A merged universal config may contain unrelated namespaces such as `[artur]`, `[bria]`, `[ladon]`, `[pano]`, `[meta]`, `[runtime]`, `[paths]`, `[objects]`, and `transports.amqp`; Oracles ignores them. Unknown fields in the Oracles namespace are rejected.

Environment expansion in relevant shared and `[oracles]` strings is:

```text
${NAME}
${NAME:-default}
```

The first requires a set variable; use the default form only for a safe non-secret default. Keep provider keys and sink credentials in environment variables.

### Root, logging, stores, HTTP, chains, and assets

| Section / parameter | Required / default | Contract |
|---|---|---|
| `version` | required, `1` | Configuration schema version. |
| `[log].level` | default `info` | `trace`, `debug`, `info`, `warn`, or `error`. |
| `[log].format` | default `json` | `json`, `pretty`, or `compact`. |
| `[stores.<id>].driver` | required | `sqlite` or feature-gated `postgres`. |
| `url` | required | SQLite: `sqlite://data/oracles.db`, `sqlite://:memory:`, or `sqlite::memory:`; PostgreSQL: `postgres://…`. |
| `migrate` | default `true` | Creates required schema/indexes on store open. |
| `connect_timeout_secs` | default `10`, > 0 | Connection-open timeout. |
| `max_connections` | default/required `1` | **Must be 1** for both drivers; pooling is not implemented. |
| `[http].user_agent` | default `oracles/0.3` | Provider HTTP User-Agent. |
| `request_timeout_secs` | default `15` | Per-request provider timeout. |
| `max_retries` | default `3` | Retries after the first provider request. |
| `retry_backoff_ms` | default `500` | Exponential provider retry base, capped at 30 seconds. |
| `[chains.<id>]` | required for referenced asset | `family`, `caip2`, optional `native_symbol`, `rpc_urls`, and `confirmations`; metadata only, no RPC reads. |
| `[assets.<id>]` | required for resolved asset | `enabled`, `chain`, `symbol`, optional `name`, `kind`, `contract`, `decimals` (≤18), optional `[x402]`. |

Asset IDs are lowercase ASCII letters, digits, underscores, and hyphens. Prefer chain-qualified IDs such as `usdc_base`, not a ticker alone. `x402` metadata is optional; helpers only calculate/format values and do not settle payments.

### Oracle, rate-table, and feed configuration

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
# asset_ids = ["eth", "usdc_base"]

[oracles.table]
name = "oracle_rates"
write_mode = "upsert"
```

| Parameter | Contract |
|---|---|
| `store`, `quote`, `refresh_secs`, `stale_after_secs` | Required. Quote is uppercase; refresh is ≥1; `stale_after_secs >= refresh_secs`. |
| `max_source_age_secs` | Optional fallback source-age limit; safety’s value takes precedence when set. |
| `max_concurrent_requests` | Default 8; bounds concurrent HTTP work for `all`/`median`. |
| `fail_fast` | Default false; stop a `run_once` call at the first asset error when true. |
| `selection` | `priority`, `all`, or `median`; defined below. |
| `asset_ids` | Optional restriction to shared assets. |
| `table.name` | Valid, non-reserved unquoted SQL identifier. |
| `table.write_mode` | `upsert` retains latest `(asset_id, quote, provider)`; `append` inserts every accepted observation. |
| `table.columns` | Optional mappings for `id`, `asset_id`, `chain_id`, `caip2`, `symbol`, `quote`, `provider`, `rate`, `source_updated_at`, `observed_at`, `expires_at`. Keys must be known and physical names unique. |

`expires_at = observed_at + stale_after_secs`. Define feeds either at `[[assets.<id>.feeds]]` or under `[oracles.assets.<id>]`; the latter replaces shared feeds for that asset. Each enabled asset needs an enabled feed.

```toml
[oracles.assets.eth]
enabled = true

[[oracles.assets.eth.feeds]]
enabled = true
provider = "coingecko_coin"
priority = 100
params = { coin_id = "ethereum" }
```

### Providers and transport profiles

`static` requires `params.rate`. `http_json` requires `url_template`, supports only `GET` and empty-body `POST`, and can have one auth header whose value is read from `auth.value_env` at fetch time.

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

Templates in URLs and paths use built-ins `{asset_id}`, `{chain_id}`, `{caip2}`, `{symbol}`, `{symbol_lower}`, `{quote}`, `{quote_lower}`, `{contract}`, `{contract_lower}`, plus each feed parameter as `{name}` and `{name_lower}`. URL substitutions are URL-encoded. Paths traverse dot-separated JSON objects only: no arrays, JSONPath filters, or escaped dotted keys. Timestamps are `rfc3339`, `unix`, or `unix_ms`; a missing timestamp remains usable unless safety requires it.

`[transports.http.<id>]` and `[transports.webhook.<id>]` references are validated and can be declared with `transport = "<id>"`. **Current runtime caveat:** provider fetching uses `[http]` defaults rather than HTTP profile overrides; webhook profile URL/method/auth/header values are not applied by the sink. Configure effective provider defaults in `[http]` and effective webhook `url_env`, headers, and POST method directly on the sink.

### Safety

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

[oracles.safety.bootstrap]
missing_previous_rate = "accept"

[oracles.safety.consensus]
min_successful_feeds = 2
max_provider_spread_pct = "3"
action = "quarantine"
```

Safety checks run source age (when both a timestamp and limit exist), min/max bounds, percentage change, then bootstrap. `max_change_pct` must be a non-negative decimal. Asset `[assets.<id>.safety]` may override enabled status, bounds, change limit, and action. `last_observed` requires enabled, recorded events; `last_accepted` consults accepted rows only. See [selection and safety contracts](#provider-selection-safety-events-outbox-and-store-contracts) for decisions.

### Events, sinks, and outbox

| Setting | Contract |
|---|---|
| `[oracles.events].enabled` | Enables audit/routing. `store` must equal `oracles.store`; independent routing is not implemented. |
| `mode` | `simple`: record then immediately deliver. `outbox`: transactionally create event/pending deliveries for later dispatch. |
| `record` | Durable audit row. Required by outbox mode and table sinks. |
| `table`, `columns` | Table and optional mappings: `id`, `event_type`, `asset_id`, `chain_id`, `symbol`, `quote`, `provider`, `previous_rate`, `candidate_rate`, `change_pct`, `action`, `reason`, `source_updated_at`, `observed_at`. |
| `sink_fail_fast` | In simple mode, sink error fails the event path when true; otherwise it is logged. |
| `[oracles.outbox].enabled` | Durable dispatch; its `store` must equal `oracles.store`. |
| Outbox settings | `table`, `dispatch_interval_secs`, `max_retries`, `retry_backoff_secs`, `request_timeout_secs`, and optional columns `id`, `event_id`, `sink`, `payload`, `status`, `attempts`, `next_attempt_at`, `delivered_at`, `last_error`. |

Routes map one recognized event type to existing sink IDs. Sinks are `log`, `table` (already-recorded-event no-op), feature-gated `telegram`, and feature-gated `webhook`. Telegram is POST-only. Webhook configuration validation can permit more methods, but runtime delivery is **POST-only**.

## Provider, selection, safety, events, outbox, and store contracts

### Selection and safety decisions

| Selection | Fetch and write behavior |
|---|---|
| `priority` | Try enabled feeds by descending priority; use first successful candidate. |
| `all` | Fetch all enabled feeds; after consensus, evaluate/write each successful candidate. |
| `median` | Fetch all enabled feeds; after consensus, evaluate the upper median candidate. |

`all` and `median` enforce `safety.consensus.min_successful_feeds`, even when safety is disabled. Their fetches may use bounded OS-thread concurrency. Consensus spread is `(max - min) / min * 100`; its action is `alert`, `quarantine`, or `reject`, never `disable_asset`. `require_multiple_providers` bootstrap needs at least two candidates, so priority cannot satisfy it.

| Safety action | Durable/active-rate implication |
|---|---|
| `alert` | Accepts the candidate and emits an event. |
| `quarantine` | Does not write an active accepted rate; records/routes its event when enabled. |
| `reject` | Does not write an active accepted rate; records/routes its event when enabled. |
| `disable_asset` | Does not write an active rate. A recorded durable event makes later refreshes skip the asset while it is the latest relevant action. Requires enabled recorded anomaly events. |

Alert cooldown suppresses delivery, not eligible event recording. The old accepted rate remains usable only until its own expiry; do not extend it after a quarantine/rejection/disable decision.

### Events and outbox

Event types are:

```text
oracle.rate_anomaly
oracle.rate_quarantined
oracle.rate_rejected
oracle.provider_failed
oracle.refresh_failed
```

Reasons include `max_change_exceeded`, `provider_spread_exceeded`, `source_timestamp_too_old`, `rate_below_min`, `rate_above_max`, `missing_previous_rate`, `provider_error`, and `parse_error`.

Decision writes and associated recorded event/outbox rows commit together; provider requests occur outside that transaction. Provider and refresh failure events have their own transaction. In simple mode, matching sinks run after recording. In outbox mode, non-table routes become pending rows in the same decision transaction.

The dispatcher reads due `pending` rows. Success marks `delivered`; failure increments attempts and reschedules `pending` until `max_retries`, then marks `dead`. Missing/invalid sink configuration makes a row dead immediately. Delivery is at-least-once: an endpoint can receive a payload before the subsequent store update fails. A `dead` row is terminal in this runtime—preserve payload and `last_error`, diagnose, obtain approval, then use a controlled database/configuration recovery process rather than deleting or blindly replaying it.

### Store contract and schema

With `migrate = true`, schema and indexes are created automatically; SQL is also under `migrations/sqlite` and `migrations/postgres`. PostgreSQL upsert requires the `(asset_id, quote, provider)` unique index created by the migrator. Before changing an existing PostgreSQL deployment from upsert to append, deliberately remove the old index:

```sql
DROP INDEX IF EXISTS oracle_rates_asset_quote_provider_uniq;
```

Table and column names must be valid non-reserved unquoted SQL identifiers. Rates contain decimal-string `rate`, provider identity, optional provider timestamp, local observation time, and expiry. Events hold the decision/audit fields. Outbox state is `pending`, `delivered`, or `dead` with attempts, next attempt, and last error.

## Outputs, database consumers, errors, and troubleshooting

Consumers must query only fresh accepted records—not events or logs. SQLite/PostgreSQL timestamp expression syntax may differ; this is the SQLite-compatible shape:

```sql
SELECT rate, provider, observed_at, expires_at
FROM oracle_rates
WHERE asset_id = 'eth'
  AND quote = 'USD'
  AND expires_at > CURRENT_TIMESTAMP
ORDER BY observed_at DESC, id DESC
LIMIT 1;
```

No row means unavailable or stale; do not reuse an expired value. In `all` mode, choose the provider/ordering policy appropriate for the consumer rather than assuming a single row.

| Symptom | Diagnose and recover safely |
|---|---|
| `check` fails | Correct the named parse, expansion, feature, identifier, reference, or incompatible event/outbox setting; rerun `check`. |
| `--once` reports failures | Inspect provider/refresh events, logs, enabled feeds, provider URL/templates/paths, timestamps, credentials (names/presence only), and fresh rows. Do not call the pass healthy because exit status is 0. |
| Quarantine/rejection | Inspect event reason, prior baseline, source/observed times, bounds, change, bootstrap, and consensus. Do not weaken safety merely to clear an alert. |
| Store open/write failure | Check feature, URL/access, migration policy, schema/mappings, and `max_connections = 1`; do not rewrite accepted rows to mask it. |
| Notification failure | Distinguish simple-mode logged delivery errors from outbox `pending`/`dead`; validate route/sink/feature/POST endpoint and receiver idempotency. |
| `dead` outbox row | It is not retried automatically. Preserve diagnostics and use approved controlled recovery; do not delete it as routine remediation. |

## Public Rust API

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
    eprintln!("attempted={}, succeeded={}, failed={}",
        summary.attempted, summary.succeeded, summary.failed);
    Ok(())
}
```

Common public entry points are `config::load_config`, `config::validate::resolve_config`, `provider::build_providers`, `store::sqlite::SqliteRateStore::open`, feature-gated `store::postgres::PostgresRateStore::open`, `Oracle::new`, `Oracle::run_once`, `Oracle::dispatch_outbox`, `engine::scheduler::run_loop`, and `engine::scheduler::request_shutdown`. Core extension traits are `provider::Provider`, `store::RateStore`, `store::OutboxStore`, and `events::sinks::EventSink`. `x402::{convert_fiat_to_asset, x402_price, format_rate, has_x402}` are available; conversion floors fractional base units.

## Deployment, reliability, security, and known runtime limitations

Deploy with a durable database, read-only configuration, environment-managed secrets, conservative freshness/safety policy, monitoring for expired/missing rows, failures, quarantines, disabled assets, and pending/dead outbox rows. Use isolated stores before approving a mutating production pass. Back up and migrate schemas deliberately; do not use manual row edits as operational recovery.

**Known runtime limitations**

| Limitation | Operational consequence |
|---|---|
| `max_connections = 1` for SQLite and PostgreSQL | No store connection pooling. |
| HTTP transport profiles are validated but not applied to provider fetching | Configure effective provider timeout/retry/user-agent in `[http]`. |
| Webhook transport profile values are not applied | Configure direct sink `url_env`, headers, and `method = "POST"`. |
| Webhook runtime is POST-only | Do not configure PUT/PATCH despite validator acceptance. |
| HTTP JSON paths | Object-dot paths only; no arrays, filters, or dotted-key escaping. |
| Events/outbox stores | Must equal `oracles.store`; independent routing is unimplemented. |
| Chain metadata | No on-chain RPC reads. |
| Scheduler/runtime | Synchronous/blocking; concurrent all/median fetches use bounded OS threads. |
| Outbox | At-least-once delivery; terminal `dead` rows need external controlled recovery. |

## Development and tests

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
```

The repository has no external-service end-to-end harness. Its integration tests cover configuration parsing/resolution, provider selection, safety, SQLite persistence, event rendering, and outbox behavior in process.

## License

MIT
