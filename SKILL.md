---
name: oracles
description: Use when operating, configuring, integrating, diagnosing, or modifying the `oracles` Rust cryptocurrency-rate service/library: provider feeds, rate safety decisions, SQLite/PostgreSQL persistence, event sinks, or durable outbox delivery. Do not use for general cryptocurrency advice, trading decisions, chain RPC operations, or unrelated application configuration.
---

# Oracles operating guide

## Purpose and non-goals

`oracles` fetches configured cryptocurrency/fiat rates, evaluates candidates against safety policy, and durably stores accepted rates, events, and outbox state. The durable store is authoritative; process memory, logs, provider responses, and notifications are not. Rates are decimal strings—never replace them with floating-point arithmetic. Consumers must use only a fresh accepted row and treat no fresh row as unavailable.

Do **not** use this service to recommend trades or prices; perform chain RPC, wallet, transfer, signing, or x402 settlement; or claim that a fetched/notified value is correct. Chain metadata and x402 helpers do not perform those operations. Do not alter production config, safety thresholds, schemas, migrations, providers, or rows without an explicit reviewed request.

## Command selection

| Intent | Command | Result | Does not prove |
|---|---|---|---|
| Binary liveness | `oracles ping` | Prints `pong`; does not load config or open a store. | DB, providers, credentials, features, or rate health. |
| Validate config | `oracles --config /absolute/path/Config.toml check` | Loads, expands, validates configuration/features; initializes logging. | DB connectivity/migration, provider reachability, or sink delivery. |
| One mutating pass | `oracles --config /absolute/path/Config.toml --once` | Opens/may migrate store, refreshes assets, then dispatches up to 50 due outbox rows. | All assets refreshed or all due rows drained. |
| Continuous service | `oracles --config /absolute/path/Config.toml` | Immediate refresh/outbox pass, then repeats at configured intervals. | Every cycle/delivery succeeded. |
| Runtime verbosity | Append `--log-level debug` (or `trace`) | Overrides runtime log level only. | A persistent config change. |

`--config` wins over `ORACLES_CONFIG`; unset/empty falls back to `Config.toml`. If combined, `ping` takes precedence, then `check`, then `--once`; issue only one mode. Use `cargo run --features <needed-features> -- --config …` only for a deliberate checkout test.

The built-in help lists `error`, `warn`, `info`, and `debug` for `--log-level`, but its parenthetical omits accepted `trace`. `trace` is valid in `[log].level` and as `--log-level trace`.

## Prerequisites and features

- Building requires Rust 1.97 or later. Default features are `cli`, `config-toml`, `http-json`, and `sqlite`.
- `postgres` enables PostgreSQL; `postgres-tls` implies it. `telegram` and `webhook` each imply `http-json`; `full` enables all optional capabilities.
- The active binary must contain every configured store/sink feature. `check` catches absent compiled features but cannot test external services.
- Both SQLite and PostgreSQL require `max_connections = 1`; pooling is not implemented.

## Safe config workflow

1. Identify deployment, absolute config path, active binary/features, target store, and required environment-variable **names**. Never expose secret values.
2. Read the existing configuration and make the smallest reviewable change. Preserve `version = 1`. Unrelated namespaces in a merged config are ignored; unknown Oracles fields are rejected.
3. Keep secrets outside TOML: `${NAME}` requires presence; `${NAME:-default}` is only for a safe non-secret default.
4. Verify references: asset → chain, enabled asset → enabled feed, feed → provider, stores, routes → enabled sinks. Asset IDs are lowercase stable IDs; `decimals <= 18`.
5. Preserve store/schema safety: events/outbox stores must equal `oracles.store`; table names must be distinct valid non-reserved SQL identifiers. Do not change `migrate`, write mode, table, or column mapping without migration and rollback plans.
6. Run `check` in the intended environment. For provider/store/sink changes, use an isolated non-production database and inspect rate, event, and outbox rows before requesting production approval.
7. After deployment, monitor fresh accepted rates, failures, quarantines/disabled assets, and pending/dead outbox rows. Retain known-good config; do not delete audit data as rollback.

## Exact commands

```bash
# Documentation of accepted arguments
oracles --help

# Does not load config or open a database
oracles ping

# Non-mutating config validation
oracles --config /absolute/path/Config.toml check

# Mutating one pass
oracles --config /absolute/path/Config.toml --once

# Long-running worker
oracles --config /absolute/path/Config.toml

# Temporary runtime override
oracles --config /absolute/path/Config.toml --log-level debug
```

`--help` exits 0. Any returned error exits 1 after writing `error: …` to stderr. `ping` writes `pong` to stdout. `check` succeeds after parsing, relevant environment expansion, validation, feature compatibility, and references—not store open/migration, credentials resolved only during fetch/delivery, network, or sink success.

When the effective log level permits `info`, successful `check` logs this message in the configured format:

```text
Config is valid.
```

When the effective log level permits `info`, the one-shot summary logged to stderr is:

```text
Refresh complete: <attempted> attempted, <succeeded> succeeded, <failed> failed
```

Under the same logging condition, if due outbox work was attempted:

```text
outbox: <attempted> attempted, <delivered> delivered, <failed> failed, <dead> dead
```

With `fail_fast = false`, a completed `--once` can exit 0 with `failed > 0`; those assets are not healthy. With `fail_fast = true`, the first refresh error exits 1. A disabled asset is skipped and counted successful, so inspect durable events/fresh rows. The loop logs refresh/outbox errors and continues; shutdown completes the current operation but does not promise an outbox drain.

## Configuration model

Required operational data is `version`, stores, chains, assets, and `[oracles]`; logging and HTTP defaults have defaults. Oracle-local `[oracles.assets.<id>]` feeds replace shared `[[assets.<id>.feeds]]` for that asset; `oracles.asset_ids` limits resolved shared assets.

- Store: `driver`, `url`, `migrate` (default true), `connect_timeout_secs` (default 10), `max_connections` (must be 1).
- HTTP: `user_agent`, `request_timeout_secs`, `max_retries` (retries after initial request), `retry_backoff_ms` (exponential, capped at 30 seconds).
- Oracle: `store`, uppercase `quote`, `refresh_secs`, `stale_after_secs >= refresh_secs`, optional `max_source_age_secs`, `max_concurrent_requests` (default 8), `fail_fast`, and selection mode.
- Rate table: `name`, `write_mode` (`upsert` latest per asset/quote/provider, or `append` every accepted observation), optional column mappings.
- `expires_at = observed_at + stale_after_secs`. Consumers must not extend it.
- HTTP JSON supports `GET` and empty-body `POST`; static feeds require `params.rate`. JSON paths are dot-separated object paths only; timestamps are `rfc3339`, `unix`, or `unix_ms`.
- Provider HTTP transport and webhook transport references are validated, but runtime does not apply their profile values. Provider fetches use `[http]`; configure webhook URL/headers/POST directly on the sink.

## Provider, selection, and safety decision contracts

| Selection | Contract |
|---|---|
| `priority` | Descending feed priority; first successful candidate. |
| `all` | Fetch/evaluate every successful enabled feed after consensus. |
| `median` | Fetch all, check consensus, evaluate upper median candidate. |

`all`/`median` enforce `consensus.min_successful_feeds` even when safety is off and can use bounded OS-thread concurrency. Consensus spread is `(max - min) / min * 100`. `require_multiple_providers` needs at least two candidates; priority cannot meet it.

Safety checks run source age (when timestamp and limit exist), bounds, percentage change, then bootstrap. `last_observed` requires enabled recorded events; `last_accepted` uses accepted rows. Asset safety can override enabled, bounds, change limit, and action.

| Action | Implication |
|---|---|
| `alert` | Accepts rate and emits event. |
| `quarantine` | Does not write an active rate; records/routes event when enabled. |
| `reject` | Does not write an active rate; records/routes event when enabled. |
| `disable_asset` | Does not write an active rate; recorded durable event makes later refreshes skip asset while latest relevant action remains disable. Requires enabled recorded anomaly events. |

Consensus cannot use `disable_asset`. Alert cooldown suppresses sink delivery, not eligible event recording. After a non-accept decision, a prior accepted rate is usable only until it expires.

## Event, sink, and outbox contracts

Event types:

```text
oracle.rate_anomaly
oracle.rate_quarantined
oracle.rate_rejected
oracle.provider_failed
oracle.refresh_failed
```

`simple` mode records then delivers matching sinks; `sink_fail_fast = false` logs a sink error without failing that event path. `outbox` requires `events.record = true`, `safety.record_anomalies = true`, and enabled outbox; it transactionally writes recorded events and pending non-table deliveries. Table sink delivery is a no-op because recording is the delivery.

Log/table sinks are always available; Telegram and webhook are feature-gated. Telegram is POST-only. **Webhook runtime is POST-only**: even if validation accepts PUT/PATCH, delivery returns an error. Webhook URL is read from its named environment variable when the sink is constructed; header expansions are then resolved. Do not rely on webhook transport-profile URL/method/auth/header values.

Decision event/outbox writes commit atomically. Provider and refresh failure events are separately transactional. Dispatcher outcomes are: `pending` → `delivered` on success; failure increments attempts and stays `pending` until `next_attempt_at`; max retries makes it `dead`. Invalid/missing sink config makes a row dead immediately. Delivery is at-least-once; receivers must tolerate duplicates.

## Outcomes, error diagnosis, and recovery

1. Run `check` first. Correct parse, environment, identifier, reference, unsafe configuration, or feature errors before a run.
2. For provider failure, verify feature, method, URL template/params, object path, timestamp format, quote, `[http]` timeout/retry, and required environment-variable names/presence. `429`, `503`, timeouts, malformed JSON, missing paths, invalid decimal, and invalid timestamp fail a candidate.
3. For quarantine/rejection, inspect event reason, stored baseline, timestamps, bounds, change, bootstrap, and consensus. Do not weaken safety merely to clear an alert.
4. For store failure, verify feature, URL/access, migration/schema, mappings, and `max_connections = 1`. Do not edit accepted rows to mask failure.
5. For notifications, distinguish simple-mode logged failure from outbox state. Verify routes, sink feature, direct webhook POST config, endpoint access, and receiver idempotency.
6. A dispatcher store-update error means external delivery may have happened while durable state is unknown; do not resend without row inspection and receiver idempotency evidence.
7. `dead` is terminal in this implementation. Preserve payload and `last_error`, diagnose, get approval, then use controlled database/configuration recovery outside the service—never silently delete or bulk-replay rows.

## Limitations

| Limitation | Consequence |
|---|---|
| Store connections | SQLite and PostgreSQL require `max_connections = 1`; no pooling. |
| Transport profiles | Validated but not applied to provider/webhook runtime settings. |
| Webhooks | Runtime POST only. |
| JSON paths | No arrays, filters, or escaped dotted keys. |
| Store routing | Events/outbox must share `oracles.store`. |
| Chain support | Metadata only; no chain RPC reads. |
| Runtime | Synchronous/blocking; all/median concurrency uses OS threads. |
| Outbox | At-least-once delivery; dead rows require controlled external recovery. |

## Prohibited actions

- Do not run `--once` or the loop against a production store merely to test config.
- Do not disable/loosen safety, expand staleness, alter selection/bootstrap/consensus, or re-enable a disabled asset without explicit approval.
- Do not place credentials in TOML, arguments, source control, event templates, or diagnostics.
- Do not configure PUT/PATCH webhooks or assume transport profiles change HTTP runtime behavior.
- Do not delete, rewrite, or routine-replay rate/event/outbox records; do not claim exactly-once delivery.
- Do not consume stale rows, treat an event as an accepted rate, or infer health from process lifetime/exit status alone.

## Verification checklist

- [ ] Correct deployment, binary features, absolute config path, and target store identified.
- [ ] Credentials remain environment-only and were not exposed.
- [ ] References, methods, paths, feature gates, and `max_connections = 1` match runtime behavior.
- [ ] Safety, bootstrap, consensus, freshness, and consumer stale-rate behavior were reviewed without unapproved weakening.
- [ ] `oracles --config /absolute/path/Config.toml check` passed in the intended environment.
- [ ] Any mutating run was approved; its exact summary, fresh accepted rows, events, and outbox state were inspected.
- [ ] Failures, quarantines, disabled assets, pending rows, and dead rows have an owner and approved recovery path.
