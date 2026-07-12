//! Pluggable event sinks for delivering oracle events.

use crate::config::ResolvedEventSink;
use crate::error::Result;

/// Stderr log sink.
pub mod log;

#[cfg(feature = "webhook")]
/// HTTP webhook sink.
pub mod webhook;

#[cfg(feature = "telegram")]
/// Telegram Bot API sink.
pub mod telegram;

/// A sink that can deliver an event payload.
pub trait EventSink {
    /// Deliver a rendered payload to this sink.
    fn deliver(&self, payload: &str) -> Result<()>;
}

/// A no-op sink that confirms delivery without external action.
/// Used for the `table` sink kind, since events are already written to
/// the event table by the engine.
struct TableSink;

impl EventSink for TableSink {
    fn deliver(&self, _payload: &str) -> Result<()> {
        // Table sink is a no-op: events are already recorded by write_event()
        Ok(())
    }
}

/// Build an [`EventSink`] from its resolved configuration.
///
/// Returns a boxed trait object for the appropriate sink kind. Fails if a
/// required feature (`webhook` or `telegram`) is not enabled.
pub fn build_sink(config: &ResolvedEventSink) -> Result<Box<dyn EventSink>> {
    match config {
        ResolvedEventSink::Log { level } => Ok(Box::new(log::LogSink::new(level.clone()))),

        ResolvedEventSink::Table => Ok(Box::new(TableSink)),

        #[cfg(feature = "webhook")]
        ResolvedEventSink::Webhook {
            url_env,
            method,
            headers,
            timeout_secs,
            ..
        } => Ok(Box::new(webhook::WebhookSink::new(
            url_env.clone(),
            method.clone(),
            headers.clone(),
            *timeout_secs,
        )?)),

        #[cfg(not(feature = "webhook"))]
        ResolvedEventSink::Webhook { .. } => Err(crate::error::Error::Config(
            "webhook sink requires the `webhook` feature".to_owned(),
        )),

        #[cfg(feature = "telegram")]
        ResolvedEventSink::Telegram {
            bot_token_env,
            chat_id_env,
            method,
            parse_mode,
            disable_web_page_preview,
            timeout_secs,
            ..
        } => Ok(Box::new(telegram::TelegramSink::new(
            bot_token_env.clone(),
            chat_id_env.clone(),
            method.clone(),
            parse_mode.clone(),
            *disable_web_page_preview,
            *timeout_secs,
        ))),

        #[cfg(not(feature = "telegram"))]
        ResolvedEventSink::Telegram { .. } => Err(crate::error::Error::Config(
            "telegram sink requires the `telegram` feature".to_owned(),
        )),
    }
}
