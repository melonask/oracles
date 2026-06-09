use crate::error::Result;
use crate::events::sinks::EventSink;

/// A simple event sink that writes to stderr with a log-level prefix.
pub struct LogSink {
    level: String,
}

impl LogSink {
    /// Create a new [`LogSink`] with the given log level (`"error"`, `"warn"`, `"info"`, `"debug"`).
    pub fn new(level: String) -> Self {
        Self { level }
    }
}

impl EventSink for LogSink {
    fn deliver(&self, payload: &str) -> Result<()> {
        match self.level.as_str() {
            "error" => crate::error!("{payload}"),
            "warn" => crate::warn!("{payload}"),
            "info" => crate::info!("{payload}"),
            "debug" => crate::debug!("{payload}"),
            "trace" => crate::trace!("{payload}"),
            _ => crate::warn!("[oracles:event] {payload}"),
        }

        Ok(())
    }
}
