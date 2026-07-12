---
name: oracles
description: Use when operating, configuring, integrating, diagnosing, or modifying the `oracles` Rust cryptocurrency-rate service/library: provider feeds, rate safety decisions, SQLite/PostgreSQL persistence, event sinks, or durable outbox delivery. Do not use for general cryptocurrency advice, trading decisions, chain RPC operations, or unrelated application configuration.
---

# Oracles operating guide

`oracles` fetches configured cryptocurrency/fiat rates, evaluates candidates
against safety policy, and durably stores accepted rates. It is not a price
authority: consumers must read a fresh accepted row from its store and treat no
fresh row as unavailable. Rates are decimal strings; never substitute floating
point arithmetic.

## Scope and non-goals

Use this skill for the service binary, its TOML configuration, Rust library API,
SQLite/PostgreSQL stores, providers, safety events, sinks, and outbox.

Do **not** use it to:

- recommend assets, prices, trades, or risk limits without an operator's policy;
- claim a rate is correct merely because it was fetched or notified;
- perform chain RPC reads, wallet operations, transfers, signing, or x402
  settlement—chain metadata and x402 helpers do not perform those operations;
- alter database rows, schemas, migrations, safety thresholds, providers, or
  production configuration without an explicit, reviewed change request;
- treat the worker's memory, logs, a notification, or a provider response as
  the source of truth.

The durable store is authoritative for accepted rates, recorded events, and
outbox state. The process is intentionally stateless between refreshes.

## Command selection matrix

Use the installed `oracles` binary for operational commands. For a checkout,
use `cargo run --features <needed features> --` only when deliberately testing
that checkout; it must be followed by `--` before oracle arguments.

| Intent | Safe command | What it does | Do not infer |
|---|---|---|---|
| Binary liveness only | `oracles ping` | Prints `pong`; reads neither config nor store. | Config, feature, credential, provider, database, or rate health. |
| Validate a proposed config | `oracles --config /absolute/path check` | Loads, expands environment references, and validates config/features; initializes logging. It does not open a store, migrate, fetch, write, or deliver. | Database connectivity, migrations, provider reachability, or sink credentials that are resolved only at delivery. |
| One bounded production-like pass | `oracles --config /absolute/path --once` | Opens the store, may migrate, fetches/evaluates each enabled asset once, persists decisions/events, then dispatches up to 50 due outbox rows if enabled. | That every asset refreshed, every notification was delivered, or all due outbox work was drained. |
| Continuous service | `oracles --config /absolute/path` | Immediately refreshes and, if enabled, dispatches up to 50 due rows; repeats refresh and dispatch on their configured intervals until shutdown is requested. | A nonzero process lifetime means all cycles succeeded; refresh and dispatch errors are logged and the loop continues. |
| Temporarily increase observability | append `--log-level debug` | Overrides only the runtime log level. | A persisted config change or additional validation. |

`--config` takes precedence over `ORACLES_CONFIG`; an unset/empty environment
variable falls back to `Config.toml`. `ping` takes precedence if combined with
other modes. `check` takes precedence over `--once`. Do not combine modes;
issue the one command that expresses the intended operation.

Before a routine `--once` or loop start, run `check` against the same absolute
config path and with the same feature build and environment. Never use `--once`
as a configuration test against a production store: it is a mutating operation.

## Config-edit workflow

1. Identify the deployment, exact config path, active binary/features, target
   store, and required environment-variable *names*. Do not expose secret
   values in commands, commits, logs, or config.
2. Read the existing config and make the smallest reviewable change. Preserve
   `version = 1`; unknown fields within Oracles-owned structures are rejected.
   A merged universal config may contain unrelated package namespaces, which are
   ignored by this package.
3. Keep secrets out of TOML. Use `${NAME}` where absence must fail validation;
   use `${NAME:-default}` only for a genuinely safe non-secret default. Provider
   auth and sink credentials are environment variable names/expansions, not
   literal credentials.
4. Verify every cross-reference: selected asset -> chain; enabled asset -> at
   least one enabled feed; feed -> provider; oracle/events/outbox store -> an
   existing store; route -> an existing enabled sink. Use lowercase, stable
   asset IDs (not a ticker alone); `decimals` must not exceed 18.
5. Preserve store safety: both supported drivers require `max_connections = 1`.
   `events.store` and `outbox.store`, when enabled, must equal `oracles.store`.
   Distinct rate/event/outbox table names and valid non-reserved SQL identifiers
   are required. Do not change `write_mode`, table/column mappings, or
   `migrate` on an existing deployment without a database migration/rollback
   plan.
6. Run `check` with the intended runtime environment. Resolve every error
   before proceeding. A passing check does not prove external connectivity.
7. For a provider, store, or sink change, first use an isolated non-production
   database and a scoped/static feed where appropriate; inspect accepted rate,
   event, and outbox rows. Obtain approval before a production `--once` or loop
   restart.
8. After deployment, monitor fresh accepted rows, failure events, and outbox
   state. Retain the previous known-good config for rollback; do not "roll back"
   by deleting audit data.

## Configuration model and supported behavior

Required root data is `version`, `[stores]`, `[chains]`, `[assets]`, and
`[oracles]`; `[log]` and `[http]` are optional. Oracle-specific feeds under
`[oracles.assets.<id>]` take precedence over shared `[[assets.<id>.feeds]]`
when that oracle asset supplies feeds. `oracles.asset_ids` limits which shared
assets are resolved.

- Stores: `sqlite` and feature-gated `postgres` are supported. `migrate = true`
  creates tables/indexes on open. In upsert mode an `(asset_id, quote, provider)`
  unique index is required; the in-process migrator creates it. Changing an
  existing PostgreSQL store from upsert to append requires deliberate removal of
  the old unique index outside the service.
- Providers: only `static` and feature-gated `http_json` exist. Static feeds
  require `params.rate`. HTTP JSON supports only `GET` and an empty-body `POST`,
  a single optional auth header from `value_env`, dot-separated object paths,
  and rate values that are JSON numbers or strings. JSON paths do not support
  arrays, filters, or dotted-key escaping. Source timestamps support `rfc3339`,
  `unix`, and `unix_ms`; a missing configured path yields no source timestamp.
- HTTP retries are attempts after the initial request. Backoff is exponential
  from `http.retry_backoff_ms` and capped at 30 seconds. A provider `transport`
  reference is validated for existence, but current fetching uses `[http]`
  defaults; do not rely on transport profile values to override provider HTTP
  behavior.
- Selection: `priority` tries enabled feeds by descending priority and uses the
  first successful candidate. `all` fetches every enabled feed and evaluates
  each successful candidate. `median` fetches all successful feeds, checks
  consensus, then evaluates the upper median candidate. `all`/`median` may use
  bounded concurrent requests. Their successful-feed minimum is always enforced
  from `safety.consensus.min_successful_feeds`, including when safety is off.
- Events: `simple` records then immediately delivers matching sinks; with
  `sink_fail_fast = false`, delivery failures are logged and do not fail that
  event path. `outbox` requires `events.record = true`,
  `safety.record_anomalies = true`, and enabled outbox; it persists events and
  pending non-table deliveries in the decision transaction. The `table` sink is
  no-op delivery because recording is the delivery. Routes recognize only
  `oracle.rate_anomaly`, `oracle.rate_quarantined`, `oracle.rate_rejected`,
  `oracle.provider_failed`, and `oracle.refresh_failed`.
- Sinks: log and table are always available; Telegram and webhook require their
  compile-time features. Telegram is POST-only. Although validation permits
  webhook `POST`, `PUT`, and `PATCH`, the runtime implements **POST only**.
  Webhook transport references are validated but their URL/method/auth/header
  profile values are not applied by the current sink; configure `url_env`,
  headers, and a POST method directly. Webhook URL values are read from the
  named environment variable at sink construction; header `${...}` values are
  expanded then. Telegram token/chat ID are read at first delivery and cached
  for that process lifetime.

## Safety decision handling

Safety is a correctness boundary, not a notification preference. Keep it
enabled unless an explicit, documented exception is approved. `expires_at` is
always `observed_at + stale_after_secs`; `stale_after_secs >= refresh_secs` is
required. Consumers must filter on `expires_at > CURRENT_TIMESTAMP` (or the
database equivalent) and never extend or reuse an expired rate.

Checks run in this order: source age (only when both a maximum and provider
timestamp exist), minimum/maximum bounds, percent change, then bootstrap.
The source-age limit in `[oracles.safety]` overrides the root oracle limit.
Asset overrides can change enabled status, bounds, change limit, and action.

| Action | Persistence/result | Operational response |
|---|---|---|
| `alert` | Accepts the candidate and emits an event. | Treat it as an accepted but anomalous rate; investigate before relying on it for high-risk flows. |
| `quarantine` | Does not write an active rate; records/routes the event when events are enabled. | Preserve the prior fresh rate only until it expires; investigate provider/input/policy. |
| `reject` | Does not write an active rate; records/routes the event when events are enabled. | Treat the candidate as unusable; repair or disable the feed through an approved config change. |
| `disable_asset` | Does not write an active rate; recorded durable event makes later refreshes skip that asset while it is the latest action. | Stop dependent use when the current rate expires; require explicit human review and a durable superseding event/config action before resuming. |

`disable_asset` requires enabled, recorded anomaly events; it is not permitted
as a consensus action. `compare_against = "last_observed"` requires recorded
events and includes the latest candidate-bearing event or accepted rate;
`last_accepted` uses accepted rates only. Use `require_multiple_providers` only
with `all` or `median` and at least two viable feeds: priority mode cannot meet
it. Consensus spread is `(max - min) / min * 100`; a consensus failure applies
its configured `alert`, `quarantine`, or `reject` before individual/median
evaluation. Alert cooldown suppresses sink delivery, not eligible event
recording.

## Outcome interpretation and recovery

- `ping` success proves only that the executable reached its liveness path.
- `check` success proves parsing, environment expansion used by config, schema
  validation, feature compatibility, and references—not opening the database,
  migrations, provider authentication, network access, or sink delivery. On
  failure, make no run attempt; correct the stated error and re-run it.
- `--once` returning an error is an incomplete pass. With `fail_fast = true`, a
  refresh error stops the pass. With the default false, the summary can report
  failed assets while the command still returns success. Any `failed > 0` means
  those assets were not refreshed successfully; inspect events/logs and current
  rows, and do not report them healthy. A successful asset can also mean it was
  skipped because of a durable `disable_asset` event; verify the event state.
- The loop runs an immediate refresh and immediate outbox pass, logs errors,
  and continues. Shutdown is cooperative: it exits after the current operation;
  it does not promise to drain all outbox work.

Decision transactions protect accepted rate writes and their recorded events/
outbox rows: commit only after the decision path succeeds; rollback on an error.
Provider fetches occur outside the write transaction. Provider/refresh failure
events are separately transactional. In outbox mode, a committed event and its
pending delivery rows survive a crash before delivery. Delivery is at-least-once:
a sink can receive a payload and the subsequent delivered-state update can fail,
so downstream receivers must tolerate duplicate notifications.

Due deliveries are read only from `pending`; success becomes `delivered`.
Failure increments attempts and either remains `pending` until
`next_attempt_at`, or becomes `dead` at `max_retries`. An invalid/missing sink
configuration makes that row dead immediately. A dispatcher store-update error
means external delivery may have happened but durable status is unknown: do not
re-send manually without inspecting the row and receiver idempotency. `dead`
rows are terminal under this implementation; preserve their payload and
`last_error`, diagnose and approve remediation, then use a controlled database
or configuration recovery procedure outside the service rather than silently
deleting/retrying rows.

## Provider and sink troubleshooting

1. Run `check`; verify the feature build for configured PostgreSQL, Telegram,
   and webhook components.
2. Verify only names/presence of required environment variables. Missing
   provider auth is discovered when the provider fetches; missing sink secrets
   are discovered on delivery. Never print their values.
3. Check exact provider method, URL template placeholders, feed parameters,
   JSON object paths, timestamp format, quote, timeout, retry policy, and
   source-age policy. `429` indicates rate limiting; adjust provider-approved
   request cadence/backoff rather than bypassing limits. `503`, timeouts,
   malformed JSON, absent paths, and invalid decimal/timestamp values all make
   the candidate fail.
4. For a rejected/quarantined candidate, inspect the event reason, prior stored
   baseline, source/observed timestamps, bounds, percent change, bootstrap, and
   consensus configuration. Do not weaken a safety control solely to clear an
   alert.
5. For store errors, confirm driver feature, URL, filesystem/database access,
   schema compatibility, `max_connections = 1`, migrations policy, and table /
   column mappings. Do not manually modify accepted rate rows to mask failures.
6. For notifications, distinguish simple-mode immediate failures from outbox
   `pending`/`dead` state. Verify route-to-sink mapping, sink feature, POST-only
   HTTP behavior, endpoint access, and recipient idempotency. Correct config or
   environment, validate it, then allow the dispatcher to process due pending
   rows.

## Prohibited actions

- Do not run `--once` or the loop against a store merely to test a config.
- Do not disable safety, enlarge staleness, loosen bounds/change/age/consensus,
  switch `alert`, or re-enable a disabled asset without explicit approval.
- Do not put credentials in TOML, command arguments, source control, event
  templates, or diagnostic output.
- Do not assume transport profile fields override HTTP provider/webhook runtime
  behavior; do not configure PUT/PATCH webhooks.
- Do not delete, rewrite, or bulk-replay rate/event/outbox records as routine
  recovery, and do not claim exactly-once notification delivery.
- Do not consume stale rows, use an event as an accepted rate, or infer success
  from a process exit alone.

## Final checklist

- [ ] Correct deployment, binary features, absolute config path, and target
  store identified.
- [ ] Credentials remain environment-only and were not exposed.
- [ ] All references, supported methods, provider paths, and feature gates
  match the current implementation.
- [ ] Safety policy, bootstrap, consensus, freshness, and consumer stale-rate
  handling were reviewed; no unapproved weakening occurred.
- [ ] `oracles --config /absolute/path check` passed in the intended environment.
- [ ] Any mutating run was explicitly approved and its refresh summary, fresh
  accepted rows, relevant events, and outbox state were inspected.
- [ ] Failures, quarantines, disabled assets, pending rows, and dead rows have
  an owner and an approved recovery path.
