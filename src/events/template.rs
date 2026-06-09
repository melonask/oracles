use crate::domain::OracleEvent;
use crate::error::{Error, Result};

/// Render an event template string with field values from an [`OracleEvent`].
///
/// Supports placeholders like `{event_type}`, `{asset_id}`, `{symbol}`,
/// `{previous_rate}`, `{candidate_rate}`, `{change_pct}`, `{action}`,
/// `{reason}`, etc. Unknown placeholders cause an error.
pub fn render_event_template(input: &str, event: &OracleEvent) -> Result<String> {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '{' {
            output.push(ch);
            continue;
        }

        let mut key = String::new();

        loop {
            match chars.next() {
                Some('}') => break,
                Some(c) => key.push(c),
                None => {
                    return Err(Error::Template(
                        "unclosed event template placeholder".to_owned(),
                    ));
                }
            }
        }

        output.push_str(&event_value(event, &key)?);
    }

    Ok(output)
}

/// Render an event template with JSON-escaped values.
///
/// Like [`render_event_template`], but all substituted values are escaped
/// for safe inclusion in JSON strings. Use this for webhook body templates
/// with `format = "json"`.
pub fn render_event_template_json(input: &str, event: &OracleEvent) -> Result<String> {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '{' {
            output.push(ch);
            continue;
        }

        let mut key = String::new();

        loop {
            match chars.next() {
                Some('}') => break,
                Some(c) => key.push(c),
                None => {
                    return Err(Error::Template(
                        "unclosed event template placeholder".to_owned(),
                    ));
                }
            }
        }

        let value = event_value(event, &key)?;
        // JSON-escape the value
        json_escape_into(&mut output, &value);
    }

    Ok(output)
}

fn json_escape_into(out: &mut String, value: &str) {
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
}

fn event_value(event: &OracleEvent, key: &str) -> Result<String> {
    let value = match key {
        "event_type" => event.event_type.as_str().to_owned(),
        "asset_id" => event.asset_id.as_str().to_owned(),
        "chain_id" => event
            .chain_id
            .as_ref()
            .map(|v| v.as_str().to_owned())
            .unwrap_or_default(),
        "symbol" => event.symbol.clone(),
        "quote" => event.quote.as_str().to_owned(),
        "provider" => event.provider.as_str().to_owned(),
        "previous_rate" => event
            .previous_rate
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        "candidate_rate" => event
            .candidate_rate
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        "change_pct" => event.change_pct.map(|v| v.to_string()).unwrap_or_default(),
        "action" => event.action.as_str().to_owned(),
        "reason" => event.reason.as_str().to_owned(),
        "source_updated_at" => event
            .source_updated_at
            .map(|v| v.to_string())
            .unwrap_or_default(),
        "observed_at" => event.observed_at.to_string(),
        other => {
            // Log a warning and leave the placeholder text as-is so a
            // typo in the template doesn't silently suppress all sink
            // notifications for that event type.
            crate::warn!(
                "unknown event template placeholder `{other}` in sink template; leaving as-is"
            );
            format!("{{{other}}}")
        }
    };

    Ok(value)
}
