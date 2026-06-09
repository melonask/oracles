//! Event system: templates, sinks, and outbox dispatcher.

/// Outbox dispatcher for reliable notification delivery.
pub mod dispatcher;
/// Pluggable event sinks (log, Telegram, webhook).
pub mod sinks;
/// Event payload template rendering.
pub mod template;
