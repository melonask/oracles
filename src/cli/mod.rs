//! Command-line interface (requires the `cli` feature).

#[cfg(feature = "cli")]
/// CLI argument parsing.
pub mod args;

#[cfg(feature = "cli")]
pub use self::args::parse_args;

#[cfg(feature = "cli")]
/// Run the CLI: parse args, load config, and optionally refresh rates.
pub fn run() -> crate::error::Result<()> {
    let args = args::parse_args()?;

    let config = crate::config::load_config(&args.config_path)?;

    // Initialise the global logger from resolved config, optionally overridden
    // by the --log-level CLI flag.
    {
        use crate::logging::{LogFormat, LogLevel, Logger};
        use std::str::FromStr;

        let level_str = args.log_level.as_deref().unwrap_or(&config.log.level);
        let level = LogLevel::from_str(level_str).map_err(|e| {
            crate::error::Error::Config(format!("invalid --log-level `{level_str}`: {e}"))
        })?;
        let format = LogFormat::from_str(&config.log.format).map_err(|e| {
            crate::error::Error::Config(format!("invalid log format in config: {e}"))
        })?;

        crate::logging::init_logger(Logger::new(level, format));
    }

    if args.check {
        crate::info!("Config is valid.");
        return Ok(());
    }

    let store_driver = config
        .stores
        .get(&config.oracles.store)
        .map(|s| s.driver.clone())
        .ok_or_else(|| {
            crate::error::Error::Config(format!("store not found: {}", config.oracles.store))
        })?;

    match store_driver {
        crate::config::StoreDriver::Sqlite => {
            #[cfg(feature = "sqlite")]
            {
                let store = crate::store::sqlite::SqliteRateStore::open(&config)?;
                let providers = crate::provider::build_providers(&config)?;
                let mut oracle = crate::engine::Oracle::new(config, store, providers);
                run_with_oracle(&mut oracle, &args)
            }
            #[cfg(not(feature = "sqlite"))]
            {
                Err(crate::error::Error::Config(
                    "sqlite store driver requires the sqlite feature".to_owned(),
                ))
            }
        }
        crate::config::StoreDriver::Postgres => {
            #[cfg(feature = "postgres")]
            {
                let store = crate::store::postgres::PostgresRateStore::open(&config)?;
                let providers = crate::provider::build_providers(&config)?;
                let mut oracle = crate::engine::Oracle::new(config, store, providers);
                run_with_oracle(&mut oracle, &args)
            }
            #[cfg(not(feature = "postgres"))]
            {
                Err(crate::error::Error::Config(
                    "postgres store driver requires the postgres feature".to_owned(),
                ))
            }
        }
    }
}

#[cfg(feature = "cli")]
fn run_with_oracle<S>(
    oracle: &mut crate::engine::Oracle<S>,
    args: &args::Args,
) -> crate::error::Result<()>
where
    S: crate::store::RateStore + crate::store::OutboxStore,
{
    if args.once {
        let summary = oracle.run_once()?;
        crate::info!(
            "Refresh complete: {} attempted, {} succeeded, {} failed",
            summary.attempted,
            summary.succeeded,
            summary.failed
        );

        // Dispatch any pending outbox deliveries after the refresh.
        if oracle.config().outbox.enabled {
            match oracle.dispatch_outbox(50) {
                Ok(outbox_summary) if outbox_summary.attempted > 0 => {
                    crate::info!(
                        "outbox: {} attempted, {} delivered, {} failed, {} dead",
                        outbox_summary.attempted,
                        outbox_summary.delivered,
                        outbox_summary.failed,
                        outbox_summary.dead,
                    );
                }
                Ok(_) => { /* no pending deliveries */ }
                Err(err) => {
                    crate::error!("outbox dispatch failed: {err}");
                    return Err(err);
                }
            }
        }
    } else {
        crate::info!(
            "Starting oracle refresh loop (interval: {}s)...",
            oracle.config().oracles.refresh_secs
        );
        crate::engine::scheduler::run_loop(oracle);
    }

    Ok(())
}
