//! Run-once example: load Config.toml, fetch rates, print summary, exit.
//!
//! Takes a path to a TOML configuration file as its sole command-line argument,
//! opens the configured SQLite store, builds providers from the config, runs a
//! single refresh cycle, and prints the outcome.
//!
//! This is essentially what `oracles --config Config.toml --once` does, but
//! demonstrated as a library usage example.
//!
//! ```sh
//! cargo run --example run_once -- Config.toml
//! ```

use std::env;
use std::process;

use oracles::Result;
use oracles::config::load_config;
use oracles::engine::Oracle;
use oracles::provider::build_providers;
use oracles::store::sqlite::SqliteRateStore;

fn run(config_path: &str) -> Result<()> {
    // 1. Load and validate the TOML configuration file.
    let config = load_config(config_path)?;

    // 2. Open the SQLite store specified in the config.
    let store = SqliteRateStore::open(&config)?;

    // 3. Build all configured provider instances.
    let providers = build_providers(&config)?;

    // 4. Create the oracle and run a single refresh cycle.
    let mut oracle = Oracle::new(config, store, providers);
    let summary = oracle.run_once()?;

    // 5. Print the refresh summary to stderr.
    eprintln!("--- Refresh Summary ---");
    eprintln!("Attempted: {}", summary.attempted);
    eprintln!("Succeeded: {}", summary.succeeded);
    eprintln!("Failed:    {}", summary.failed);

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // The first arg is the binary name; we need at least one more for the
    // config path.
    if args.len() < 2 {
        let prog = args.first().map(String::as_str).unwrap_or("run_once");
        eprintln!("Usage: {prog} <Config.toml>");
        process::exit(1);
    }

    if let Err(e) = run(&args[1]) {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}
