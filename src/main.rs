#![allow(missing_docs)]

#[cfg(feature = "cli")]
fn main() {
    if let Err(err) = oracles::cli::run() {
        match err {
            oracles::Error::HelpRequested => {
                print_help();
                std::process::exit(0);
            }
            other => {
                eprintln!("error: {other}");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(feature = "cli")]
fn print_help() {
    println!(
        "\
oracles — multi-source oracle rate fetcher

Usage:
  oracles [OPTIONS]

Options:
  --config <path>      Path to config file (default: Config.toml)
  --check              Validate config and exit
  --once               Fetch rates once and exit
  --log-level <level>  Override log level (error, warn, info, debug)
  -h, --help           Show help

Examples:
  oracles
  oracles --config myconfig.toml
  oracles --config myconfig.toml --check
  oracles --config myconfig.toml --once
  oracles --config myconfig.toml --log-level debug
"
    );
}

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("the `cli` feature is disabled");
    std::process::exit(1);
}
