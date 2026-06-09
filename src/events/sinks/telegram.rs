use crate::error::{Error, Result};
use crate::events::sinks::EventSink;
use std::time::Duration;

/// A Telegram Bot API event sink.
///
/// Sends rendered event payloads as Telegram messages using the Bot API.
/// The bot token and chat ID are read from environment variables specified
/// in the sink configuration. Values are cached once on first use via
/// [`std::sync::OnceLock`].
pub struct TelegramSink {
    bot_token_env: String,
    chat_id_env: String,
    method: String,
    parse_mode: Option<String>,
    link_preview_options: Option<String>,
    timeout_secs: u64,
    cached_token: std::sync::OnceLock<String>,
    cached_chat_id: std::sync::OnceLock<String>,
}

impl TelegramSink {
    /// Create a new [`TelegramSink`].
    ///
    /// `bot_token_env` and `chat_id_env` are environment variable names
    /// (not the actual values). Values are resolved at delivery time.
    pub fn new(
        bot_token_env: String,
        chat_id_env: String,
        method: String,
        parse_mode: Option<String>,
        disable_web_page_preview: bool,
        timeout_secs: u64,
    ) -> Self {
        let link_preview_options = if disable_web_page_preview {
            Some(r#"{"is_disabled":true}"#.to_owned())
        } else {
            None
        };
        Self {
            bot_token_env,
            chat_id_env,
            method,
            parse_mode,
            link_preview_options,
            timeout_secs,
            cached_token: std::sync::OnceLock::new(),
            cached_chat_id: std::sync::OnceLock::new(),
        }
    }
}

impl EventSink for TelegramSink {
    fn deliver(&self, payload: &str) -> Result<()> {
        if self.method != "POST" {
            return Err(Error::Provider(format!(
                "telegram sink currently supports only POST, got `{}`",
                self.method
            )));
        }

        // Resolve env vars with OnceLock caching
        let token = self
            .cached_token
            .get_or_init(|| std::env::var(&self.bot_token_env).unwrap_or_default());

        let chat_id = self
            .cached_chat_id
            .get_or_init(|| std::env::var(&self.chat_id_env).unwrap_or_default());

        let token = if token.is_empty() {
            return Err(Error::Env(format!(
                "missing Telegram bot token environment variable: {}",
                self.bot_token_env
            )));
        } else {
            token
        };

        let chat_id = if chat_id.is_empty() {
            return Err(Error::Env(format!(
                "missing Telegram chat ID environment variable: {}",
                self.chat_id_env
            )));
        } else {
            chat_id
        };

        // Redacted URL for error messages to avoid token leak.
        let redacted_url = "https://api.telegram.org/bot<redacted>/sendMessage".to_string();
        let url = format!("https://api.telegram.org/bot{token}/sendMessage");

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(self.timeout_secs)))
            .build()
            .into();

        let request = agent.post(&url).header("Content-Type", "application/json");

        let body = telegram_body(
            chat_id,
            payload,
            self.parse_mode.as_deref(),
            self.link_preview_options.as_deref(),
        );

        request.send(&body).map_err(|err| {
            Error::Provider(format!(
                "Telegram delivery failed to `{redacted_url}`: {err}"
            ))
        })?;

        Ok(())
    }
}

fn telegram_body(
    chat_id: &str,
    text: &str,
    parse_mode: Option<&str>,
    link_preview_options: Option<&str>,
) -> String {
    let mut body = String::new();

    body.push('{');
    body.push_str("\"chat_id\":");
    push_json_string(&mut body, chat_id);
    body.push(',');
    body.push_str("\"text\":");
    push_json_string(&mut body, text);

    if let Some(lpo) = link_preview_options {
        body.push(',');
        body.push_str("\"link_preview_options\":");
        body.push_str(lpo);
    }

    if let Some(parse_mode) = parse_mode {
        body.push(',');
        body.push_str("\"parse_mode\":");
        push_json_string(&mut body, parse_mode);
    }

    body.push('}');
    body
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');

    for ch in value.chars() {
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

    out.push('"');
}
