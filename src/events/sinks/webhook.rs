use crate::config::env::expand_env;
use crate::error::{Error, Result};
use crate::events::sinks::EventSink;
use std::collections::BTreeMap;
use std::time::Duration;

/// An HTTP webhook event sink.
///
/// Sends rendered event payloads to a configured webhook URL via HTTP POST.
/// Environment variables in headers and URL are resolved once at construction
/// time and cached for subsequent deliveries.
pub struct WebhookSink {
    url: String,
    method: String,
    headers: BTreeMap<String, String>,
    timeout_secs: u64,
}

impl WebhookSink {
    /// Create a new [`WebhookSink`].
    ///
    /// `url_env` is the environment variable name holding the webhook URL.
    /// `headers` are custom HTTP headers with optional `${VAR}` expansion.
    /// All environment variable references are resolved once at construction.
    pub fn new(
        url_env: String,
        method: String,
        headers: BTreeMap<String, String>,
        timeout_secs: u64,
    ) -> Result<Self> {
        let url = std::env::var(&url_env).map_err(|_| {
            Error::Env(format!(
                "missing webhook URL environment variable: {}",
                url_env
            ))
        })?;

        let mut resolved_headers = BTreeMap::new();
        for (key, value) in &headers {
            resolved_headers.insert(key.clone(), expand_env(value)?);
        }

        Ok(Self {
            url,
            method,
            headers: resolved_headers,
            timeout_secs,
        })
    }
}

impl EventSink for WebhookSink {
    fn deliver(&self, payload: &str) -> Result<()> {
        if self.method != "POST" {
            return Err(Error::Provider(format!(
                "webhook sink currently supports only POST, got `{}`",
                self.method
            )));
        }

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(self.timeout_secs)))
            .build()
            .into();

        let mut request = agent.post(&self.url);

        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        request.send(payload).map_err(|err| {
            Error::Provider(format!(
                "webhook delivery failed to `{}`: {err}",
                redact_url(&self.url)
            ))
        })?;

        Ok(())
    }
}

fn redact_url(url: &str) -> String {
    if url.len() > 120 {
        format!("{}…", &url[..120])
    } else {
        url.to_owned()
    }
}
