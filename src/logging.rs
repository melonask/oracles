//! Minimal, zero-dependency logger with JSON, Pretty, and Compact formats.
//!
//! Does not use external logging crates. Configured via [`crate::logging::init_logger`] and
//! invoked through the convenience macros: `trace!`, `debug!`, `info!`,
//! `warn!`, `error!`.

use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Log severity level, in increasing order of importance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Trace-level diagnostic information.
    Trace = 0,
    /// Debug-level diagnostic information.
    Debug = 1,
    /// General informational messages.
    Info = 2,
    /// Warnings that do not prevent operation.
    Warn = 3,
    /// Error conditions.
    Error = 4,
}

impl LogLevel {
    /// Lowercase string representation.
    fn as_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }

    /// Uppercase string representation (for Pretty format).
    fn as_upper_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "trace" => Ok(LogLevel::Trace),
            "debug" => Ok(LogLevel::Debug),
            "info" => Ok(LogLevel::Info),
            "warn" => Ok(LogLevel::Warn),
            "error" => Ok(LogLevel::Error),
            other => Err(format!("unknown log level: {other}")),
        }
    }
}

/// Output format for log messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogFormat {
    /// `{"level":"info","ts":"...","msg":"..."}` (one JSON object per line).
    Json,
    /// `[INFO] message`
    Pretty,
    /// `INFO message`
    Compact,
}

impl FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "json" => Ok(LogFormat::Json),
            "pretty" => Ok(LogFormat::Pretty),
            "compact" => Ok(LogFormat::Compact),
            other => Err(format!("unknown log format: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Logger
// ---------------------------------------------------------------------------

/// Configured logger that writes to stderr.
pub struct Logger {
    level: LogLevel,
    format: LogFormat,
}

impl Logger {
    /// Create a new logger with the given minimum level and output format.
    pub fn new(level: LogLevel, format: LogFormat) -> Self {
        Self { level, format }
    }

    /// Return `true` if messages at `level` should be emitted.
    pub fn enabled(&self, level: LogLevel) -> bool {
        level >= self.level
    }

    /// Emit a log message at the given level.
    ///
    /// Silently drops the message if `level` is below the configured minimum.
    pub fn log(&self, level: LogLevel, msg: &str) {
        if !self.enabled(level) {
            return;
        }

        match self.format {
            LogFormat::Json => {
                let ts = OffsetDateTime::now_utc()
                    .format(&Rfc3339)
                    .unwrap_or_else(|_| String::new());
                let escaped_msg = json_escape(msg);
                eprintln!(
                    "{{\"level\":\"{}\",\"ts\":\"{ts}\",\"msg\":\"{escaped_msg}\"}}",
                    level.as_str()
                );
            }
            LogFormat::Pretty => {
                eprintln!("[{}] {msg}", level.as_upper_str());
            }
            LogFormat::Compact => {
                eprintln!("{} {msg}", level.as_upper_str());
            }
        }
    }

    // Convenience methods ---------------------------------------------------

    /// Log at TRACE level.
    pub fn trace(&self, msg: &str) {
        self.log(LogLevel::Trace, msg);
    }

    /// Log at DEBUG level.
    pub fn debug(&self, msg: &str) {
        self.log(LogLevel::Debug, msg);
    }

    /// Log at INFO level.
    pub fn info(&self, msg: &str) {
        self.log(LogLevel::Info, msg);
    }

    /// Log at WARN level.
    pub fn warn(&self, msg: &str) {
        self.log(LogLevel::Warn, msg);
    }

    /// Log at ERROR level.
    pub fn error(&self, msg: &str) {
        self.log(LogLevel::Error, msg);
    }
}

// ---------------------------------------------------------------------------
// Global logger
// ---------------------------------------------------------------------------

/// Global logger instance, initialised via [`init_logger`].
pub static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Set the global logger.
///
/// Subsequent calls have no effect (the first call wins).
pub fn init_logger(logger: Logger) {
    let _ = LOGGER.set(logger);
}

// ---------------------------------------------------------------------------
// Convenience macros
// ---------------------------------------------------------------------------

/// Log a formatted message at TRACE level via the global logger.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {{
        if let Some(logger) = $crate::logging::LOGGER.get() {
            logger.log($crate::logging::LogLevel::Trace, &format!($($arg)*));
        }
    }};
}

/// Log a formatted message at DEBUG level via the global logger.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {{
        if let Some(logger) = $crate::logging::LOGGER.get() {
            logger.log($crate::logging::LogLevel::Debug, &format!($($arg)*));
        }
    }};
}

/// Log a formatted message at INFO level via the global logger.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {{
        if let Some(logger) = $crate::logging::LOGGER.get() {
            logger.log($crate::logging::LogLevel::Info, &format!($($arg)*));
        }
    }};
}

/// Log a formatted message at WARN level via the global logger.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        if let Some(logger) = $crate::logging::LOGGER.get() {
            logger.log($crate::logging::LogLevel::Warn, &format!($($arg)*));
        }
    }};
}

/// Log a formatted message at ERROR level via the global logger.
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
        if let Some(logger) = $crate::logging::LOGGER.get() {
            logger.log($crate::logging::LogLevel::Error, &format!($($arg)*));
        }
    }};
}

// Re-export macros so they are reachable as `crate::logging::info!` etc.
pub use debug;
pub use error;
pub use info;
pub use trace;
// `warn` conflicts with the built-in `#[warn(...)]` attribute, so it is
// re-exported under `log_warn` instead. Use `crate::warn!()` for direct
// access, or `use crate::logging::log_warn as warn;` in your module.
pub use crate::warn as log_warn;

/// Escape a string for safe inclusion in a JSON string value.
///
/// Handles double quotes, backslashes, and control characters.
fn json_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use core::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}
