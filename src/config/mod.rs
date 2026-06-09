//! Configuration loading, validation, and resolution.
//!
//! The config pipeline reads a TOML file (or programmatic [`RawConfig`]),
//! resolves defaults, validates cross-references, expands environment
//! variable placeholders, and produces a fully-validated [`ResolvedConfig`].

/// Environment variable expansion utilities.
pub mod env;
/// Config file loading (TOML support with the `config-toml` feature).
pub mod load;
/// Raw deserialization types for TOML config files.
pub mod raw;
/// Resolved (validated) configuration types.
pub mod resolved;
/// Config validation and resolution logic.
pub mod validate;

pub use self::load::load_config;
pub use self::resolved::{
    BootstrapAction, CompareAgainst, EventMode, ProviderKind, ResolvedAsset, ResolvedChain,
    ResolvedConfig, ResolvedConsensusConfig, ResolvedEventColumns, ResolvedEventRoute,
    ResolvedEventSink, ResolvedEventsConfig, ResolvedFeed, ResolvedHttpConfig, ResolvedLogConfig,
    ResolvedOraclesConfig, ResolvedOutboxColumns, ResolvedOutboxConfig, ResolvedProvider,
    ResolvedProviderAuth, ResolvedRateColumns, ResolvedRateTableConfig, ResolvedSafetyConfig,
    ResolvedStoreConfig, ResolvedX402Config, SelectionMode, StoreDriver, WriteMode,
};
