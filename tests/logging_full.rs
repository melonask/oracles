#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(missing_docs)]

use oracles::logging::{LogFormat, LogLevel, Logger};
use std::str::FromStr;

#[test]
fn log_level_from_str_valid() {
    assert_eq!(LogLevel::from_str("trace").unwrap(), LogLevel::Trace);
    assert_eq!(LogLevel::from_str("debug").unwrap(), LogLevel::Debug);
    assert_eq!(LogLevel::from_str("info").unwrap(), LogLevel::Info);
    assert_eq!(LogLevel::from_str("warn").unwrap(), LogLevel::Warn);
    assert_eq!(LogLevel::from_str("error").unwrap(), LogLevel::Error);
}

#[test]
fn log_level_from_str_invalid() {
    assert!(LogLevel::from_str("bogus").is_err());
    assert!(LogLevel::from_str("INFO").is_err());
    assert!(LogLevel::from_str("").is_err());
}

#[test]
fn log_format_from_str_valid() {
    assert_eq!(LogFormat::from_str("json").unwrap(), LogFormat::Json);
    assert_eq!(LogFormat::from_str("pretty").unwrap(), LogFormat::Pretty);
    assert_eq!(LogFormat::from_str("compact").unwrap(), LogFormat::Compact);
}

#[test]
fn log_format_from_str_invalid() {
    assert!(LogFormat::from_str("xml").is_err());
    assert!(LogFormat::from_str("").is_err());
}

#[test]
fn logger_enabled_respects_level() {
    let logger = Logger::new(LogLevel::Warn, LogFormat::Pretty);
    assert!(!logger.enabled(LogLevel::Trace));
    assert!(!logger.enabled(LogLevel::Debug));
    assert!(!logger.enabled(LogLevel::Info));
    assert!(logger.enabled(LogLevel::Warn));
    assert!(logger.enabled(LogLevel::Error));
}

#[test]
fn json_log_escape_quotes_and_backslashes() {
    // Test that the json_escape function handles special characters.
    // We test through the log functionality indirectly by verifying no panic.
    let logger = Logger::new(LogLevel::Info, LogFormat::Json);
    // These messages should not panic (they contain quotes and backslashes).
    logger.log(LogLevel::Error, "msg with \"quotes\" and \\ backslash");
    logger.log(LogLevel::Info, "line1\nline2\rline3\ttab");
    logger.log(LogLevel::Warn, "emoji: 😀");
}

#[test]
fn logger_level_ordering() {
    assert!(LogLevel::Trace < LogLevel::Debug);
    assert!(LogLevel::Debug < LogLevel::Info);
    assert!(LogLevel::Info < LogLevel::Warn);
    assert!(LogLevel::Warn < LogLevel::Error);
}

#[test]
fn logger_convenience_methods() {
    let logger = Logger::new(LogLevel::Trace, LogFormat::Compact);
    // These should not panic.
    logger.trace("trace message");
    logger.debug("debug message");
    logger.info("info message");
    logger.warn("warn message");
    logger.error("error message");
}
