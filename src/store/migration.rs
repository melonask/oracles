//! Database migration support.
//!
//! This module is reserved for a future versioned migration runner.
//!
//! ## Current migration strategy
//!
//! Migrations currently run inline in each store's constructor (e.g.
//! [`SqliteRateStore::open`], [`PostgresRateStore::open`]) when the
//! `store.migrate` configuration flag is set to `true`.
//!
//! Each store uses `CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT EXISTS`
//! statements, so migrations are idempotent and safe to run on every start.
//!
//! ## Supported backends
//!
//! | Backend   | Feature flag | Module                       |
//! |-----------|-------------|------------------------------|
//! | SQLite    | `sqlite`    | [`crate::store::sqlite`]     |
//! | PostgreSQL| `postgres`  | [`crate::store::postgres`]   |
//!
//! ## Table schemas
//!
//! All backends maintain three tables:
//!
//! - **rates** — stores accepted rate records with `(asset_id, quote, provider)`
//!   as the primary key and upsert semantics.
//! - **events** — stores audit event rows with an auto-incrementing primary key.
//! - **outbox** — stores pending/delivered/dead outbox deliveries for
//!   transactional outbox dispatch.
//!
//! Each table has supporting indexes on the most common query patterns:
//!
//! - Rates: `(asset_id, quote)` and `(expires_at)`
//! - Events: `(asset_id, quote)` and `(observed_at)`
//! - Outbox: `(status, next_attempt_at)`
