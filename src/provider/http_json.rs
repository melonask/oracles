use crate::config::{ResolvedAsset, ResolvedFeed, ResolvedProvider};
use crate::domain::{CandidateRate, ProviderId, RateAmount};
use crate::error::{Error, Result};
use crate::provider::json_path::get_path;
use crate::provider::template::{TemplateVars, render_template};
use crate::provider::{Provider, ProviderContext};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration as StdDuration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// A provider that fetches rates from an HTTP JSON API.
///
/// The response is parsed using JSON path expressions to extract the rate
/// and optional source timestamp. URL templates support `{placeholder}`
/// substitution from asset and feed metadata.
pub struct HttpJsonProvider {
    id: ProviderId,
}

impl HttpJsonProvider {
    /// Create a new [`HttpJsonProvider`] with the given ID.
    pub fn new(id: ProviderId) -> Self {
        Self { id }
    }
}

impl Provider for HttpJsonProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn fetch(
        &self,
        asset: &ResolvedAsset,
        feed: &ResolvedFeed,
        provider: &ResolvedProvider,
        ctx: &ProviderContext,
    ) -> Result<CandidateRate> {
        let vars = template_vars(asset, feed, ctx);

        let url_template = provider.url_template.as_ref().ok_or_else(|| {
            Error::Provider(format!(
                "http_json provider `{}` requires url_template",
                self.id.as_str()
            ))
        })?;

        let url = render_template(url_template, &vars)?;

        let body = fetch_url(provider, ctx, &url)?;

        parse_http_json_candidate(asset, feed, provider, ctx, &body)
    }
}

fn fetch_url(provider: &ResolvedProvider, ctx: &ProviderContext, url: &str) -> Result<String> {
    let method = provider.method.as_deref().unwrap_or("GET");

    let mut last_error = None;

    for attempt in 0..=ctx.max_retries {
        match fetch_url_once(provider, ctx, url, method) {
            Ok(body) => return Ok(body),
            Err(err) => {
                last_error = Some(err);

                if attempt < ctx.max_retries && ctx.retry_backoff_ms > 0 {
                    // Cap backoff at 30 seconds to prevent excessive delays and overflow.
                    let backoff = ctx
                        .retry_backoff_ms
                        .checked_shl(attempt)
                        .unwrap_or(u64::MAX)
                        .min(30_000);
                    std::thread::sleep(StdDuration::from_millis(backoff));
                }
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| Error::Provider("HTTP request failed without an error".to_owned())))
}

fn fetch_url_once(
    provider: &ResolvedProvider,
    ctx: &ProviderContext,
    url: &str,
    method: &str,
) -> Result<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(StdDuration::from_secs(ctx.request_timeout_secs)))
        .build()
        .into();

    // Build the request (GET and POST have different type states in ureq 3).
    let response_result = match method {
        "GET" => {
            let mut req = agent.get(url).header("User-Agent", ctx.user_agent.as_str());
            if let Some(auth) = &provider.auth {
                let value = std::env::var(&auth.value_env).map_err(|_| {
                    Error::Env(format!(
                        "missing environment variable for provider auth: {}",
                        auth.value_env
                    ))
                })?;
                req = req.header(&auth.header, &value);
            }
            req.call()
        }
        "POST" => {
            let mut req = agent
                .post(url)
                .header("User-Agent", ctx.user_agent.as_str());
            if let Some(auth) = &provider.auth {
                let value = std::env::var(&auth.value_env).map_err(|_| {
                    Error::Env(format!(
                        "missing environment variable for provider auth: {}",
                        auth.value_env
                    ))
                })?;
                req = req.header(&auth.header, &value);
            }
            req.send("")
        }
        _ => {
            return Err(Error::Provider(format!(
                "http_json supports GET and POST, got `{method}`"
            )));
        }
    };

    let mut response = response_result.map_err(|err| {
        // Check for specific HTTP status in the ureq error.
        if let ureq::Error::StatusCode(code) = &err {
            let msg = match code {
                429 => {
                    format!("HTTP {code} Too Many Requests — consider increasing retry_backoff_ms")
                }
                503 => format!("HTTP {code} Service Unavailable"),
                _ => format!("HTTP {code}"),
            };
            return Error::Provider(format!(
                "provider request failed for URL `{}`: {msg}",
                redact_url(url)
            ));
        }
        Error::Provider(format!(
            "HTTP request failed for provider URL `{}`: {err}",
            redact_url(url)
        ))
    })?;

    response.body_mut().read_to_string().map_err(|err| {
        Error::Provider(format!(
            "failed to read HTTP response body from `{}`: {err}",
            redact_url(url)
        ))
    })
}

/// Parse a raw HTTP JSON response body into a [`CandidateRate`].
///
/// Uses the provider's configured JSON path expressions to extract the rate
/// and source timestamp from the response. Template variables from the
/// asset, feed, and context are available for path substitution.
pub fn parse_http_json_candidate(
    asset: &ResolvedAsset,
    feed: &ResolvedFeed,
    provider: &ResolvedProvider,
    ctx: &ProviderContext,
    body: &str,
) -> Result<CandidateRate> {
    let vars = template_vars(asset, feed, ctx);

    let json: Value = serde_json::from_str(body)
        .map_err(|err| Error::Provider(format!("failed to parse JSON response: {err}")))?;

    let rate_path_template = provider.rate_path.as_ref().ok_or_else(|| {
        Error::Provider(format!(
            "http_json provider `{}` requires paths.rate",
            provider.id.as_str()
        ))
    })?;

    let rate_path = render_template(rate_path_template, &vars)?;

    let rate_value = get_path(&json, &rate_path).ok_or_else(|| {
        Error::JsonPath(format!(
            "rate path `{rate_path}` not found for provider `{}`",
            provider.id.as_str()
        ))
    })?;

    let rate = json_value_to_rate(rate_value)?;

    let source_updated_at = match &provider.source_updated_at_path {
        Some(path_template) => {
            let path = render_template(path_template, &vars)?;

            match get_path(&json, &path) {
                Some(value) => Some(parse_source_timestamp(
                    value,
                    provider.source_updated_at_format.as_deref(),
                )?),
                None => None,
            }
        }
        None => None,
    };

    Ok(CandidateRate {
        asset_id: asset.id.clone(),
        chain_id: asset.chain_id.clone(),
        caip2: asset.caip2.clone(),
        symbol: asset.symbol.clone(),
        quote: ctx.quote.clone(),
        provider: provider.id.clone(),
        rate,
        source_updated_at,
        observed_at: ctx.observed_at,
    })
}

fn template_vars(
    asset: &ResolvedAsset,
    feed: &ResolvedFeed,
    ctx: &ProviderContext,
) -> TemplateVars {
    let mut vars = BTreeMap::new();

    vars.insert("asset_id".to_owned(), asset.id.as_str().to_owned());
    vars.insert("chain_id".to_owned(), asset.chain_id.as_str().to_owned());
    vars.insert("caip2".to_owned(), asset.caip2.clone());
    vars.insert("symbol".to_owned(), asset.symbol.clone());
    vars.insert("symbol_lower".to_owned(), asset.symbol.to_ascii_lowercase());
    vars.insert("quote".to_owned(), ctx.quote.as_str().to_owned());
    vars.insert("quote_lower".to_owned(), ctx.quote.lower());

    if let Some(contract) = &asset.contract {
        vars.insert("contract".to_owned(), contract.clone());
        vars.insert("contract_lower".to_owned(), contract.to_ascii_lowercase());
    }

    for (key, value) in &feed.params {
        vars.insert(key.clone(), value.clone());
        vars.insert(format!("{key}_lower"), value.to_ascii_lowercase());
    }

    vars
}

fn json_value_to_rate(value: &Value) -> Result<RateAmount> {
    match value {
        Value::Number(number) => RateAmount::parse(&number.to_string()),
        Value::String(text) => RateAmount::parse(text),
        other => Err(Error::Provider(format!(
            "rate value must be number or string, got: {other}"
        ))),
    }
}

fn parse_source_timestamp(value: &Value, format: Option<&str>) -> Result<OffsetDateTime> {
    let format = format.unwrap_or("rfc3339");

    match format {
        "rfc3339" => {
            let text = value
                .as_str()
                .ok_or_else(|| Error::Provider("rfc3339 timestamp must be a string".to_owned()))?;

            OffsetDateTime::parse(text, &Rfc3339).map_err(|err| {
                Error::Provider(format!("failed to parse RFC3339 timestamp `{text}`: {err}"))
            })
        }
        "unix" => {
            let seconds = json_value_to_i64(value)?;

            OffsetDateTime::from_unix_timestamp(seconds).map_err(|err| {
                Error::Provider(format!("failed to parse unix timestamp `{seconds}`: {err}"))
            })
        }
        "unix_ms" => {
            let millis = json_value_to_i64(value)?;
            let seconds = millis / 1000;

            OffsetDateTime::from_unix_timestamp(seconds).map_err(|err| {
                Error::Provider(format!(
                    "failed to parse unix_ms timestamp `{millis}`: {err}"
                ))
            })
        }
        other => Err(Error::Provider(format!(
            "unsupported timestamp format: {other}"
        ))),
    }
}

fn json_value_to_i64(value: &Value) -> Result<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .ok_or_else(|| Error::Provider(format!("timestamp number is not an i64: {number}"))),
        Value::String(text) => text
            .parse::<i64>()
            .map_err(|_| Error::Provider(format!("timestamp string is not an i64: {text}"))),
        other => Err(Error::Provider(format!(
            "timestamp must be number or string, got: {other}"
        ))),
    }
}

fn redact_url(url: &str) -> String {
    // Simple redaction. Most provider secrets should be in headers, not URLs.
    if url.len() > 180 {
        format!("{}…", &url[..180])
    } else {
        url.to_owned()
    }
}
