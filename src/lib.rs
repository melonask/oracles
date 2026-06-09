//! Stateless cryptocurrency rate oracle library.
//!
//! `oracles` is a config-driven cryptocurrency price oracle that fetches
//! exchange rates from multiple providers (static, HTTP JSON APIs), validates
//! them through a safety engine (rate bounds, percent-change limits, source
//! age), and persists accepted rates to a SQLite or Postgres store. It also
//! includes an event system with pluggable sinks (log, Telegram, webhook) and
//! an outbox dispatcher for reliable delivery.
//!
//! # Features
//!
//! - `cli` — Command-line interface (`oracles --config Config.toml`).
//! - `sqlite` — SQLite store backend.
//! - `http-json` — HTTP JSON provider support.
//! - `config-toml` — TOML config file parsing.
//! - `postgres` — PostgreSQL store backend.
//! - `telegram` — Telegram event sink.
//! - `webhook` — Webhook event sink.
//! - `outbox` — Compatibility feature (no-op). Outbox delivery is always
//!   available when events are enabled.
//! - `full` — Meta-feature that enables all optional features
//!   (`cli`, `config-toml`, `http-json`, `sqlite`, `postgres`, `postgres-tls`,
//!   `telegram`, `webhook`, `outbox`). The official Docker image is built with
//!   this feature set.
//!
//! # Quick start
//!
//! ```sh
//! oracles --config Config.toml --once
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Configuration loading, validation, and resolution.
pub mod config;
/// Domain types: identifiers, rates, events, and decisions.
pub mod domain;
/// Oracle engine that orchestrates fetching, safety evaluation, and persistence.
pub mod engine;
/// Error and result types for the entire crate.
pub mod error;
/// Event system: templates, sinks, and outbox dispatcher.
pub mod events;
/// Simple stderr logger with JSON/Pretty/Compact formats.
pub mod logging;
/// Rate provider abstraction and implementations.
pub mod provider;
/// Safety engine for rate validation.
pub mod safety;
/// Persistence abstraction and store implementations.
pub mod store;
/// X402 (HTTP 402 Payment Required) helpers: pricing and formatting.
pub mod x402;

#[cfg(feature = "cli")]
/// Command-line interface.
pub mod cli;

/// Convenience re-exports of the most commonly used types.
pub mod prelude;

pub use crate::engine::Oracle;
pub use crate::error::{Error, Result};
