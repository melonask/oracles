use crate::error::{Error, Result};

/// Parsed command-line arguments.
pub struct Args {
    /// Path to the TOML configuration file.
    pub config_path: String,
    /// If true, fetch rates once and exit.
    pub once: bool,
    /// If true, validate the config and exit without fetching.
    pub check: bool,
    /// If true, print "pong" and exit without loading config.
    pub ping: bool,
    /// Override the log level from config (e.g., "error", "warn", "info", "debug").
    pub log_level: Option<String>,
}

/// Parse command-line arguments from `std::env::args()`.
///
/// Supports `--config <path>`, `--once`, `--check`, `--log-level <level>`,
/// and `--help`/`-h`.
/// Returns [`Error::HelpRequested`] for help flags, which the caller
/// should handle by printing usage and exiting.
pub fn parse_args() -> Result<Args> {
    let mut config_path = None;
    let mut once = false;
    let mut check = false;
    let mut ping = false;
    let mut log_level = None;

    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Config("--config requires a value".to_owned()))?;
                if value.starts_with("--") {
                    return Err(Error::Config(format!(
                        "--config value must not start with --, got: {value}"
                    )));
                }
                config_path = Some(value);
            }
            "--once" => once = true,
            "--check" => check = true,
            "ping" | "--ping" => ping = true,
            "--log-level" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Config("--log-level requires a value".to_owned()))?;
                if value.starts_with("--") {
                    return Err(Error::Config(format!(
                        "--log-level value must not start with --, got: {value}"
                    )));
                }
                log_level = Some(value);
            }
            "--help" | "-h" => return Err(Error::HelpRequested),
            other => return Err(Error::UnknownArgument(other.to_owned())),
        }
    }

    Ok(Args {
        config_path: config_path.unwrap_or_else(|| "Config.toml".to_owned()),
        once,
        check,
        ping,
        log_level,
    })
}
