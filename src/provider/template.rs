use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// A map of template variable names to their string values.
///
/// Used by [`render_template`] for `{placeholder}` substitution in URL
/// templates and other config strings.
pub type TemplateVars = BTreeMap<String, String>;

/// Render a string with `{placeholder}` substitution from a variable map.
///
/// Placeholders are enclosed in curly braces (e.g., `{asset_id}`). Values are
/// percent-encoded for safe URL inclusion (except `/` which is left as-is).
/// Unknown placeholders cause an error. Unclosed braces also cause an error.
pub fn render_template(input: &str, vars: &TemplateVars) -> Result<String> {
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
                    return Err(Error::Template("unclosed template placeholder".to_owned()));
                }
            }
        }

        let value = vars
            .get(&key)
            .ok_or_else(|| Error::Template(format!("unknown template placeholder: {key}")))?;

        // URL-encode the value, preserving `/` for path segments.
        output.push_str(&url_encode(value));
    }

    Ok(output)
}

/// Percent-encode a string for use in URLs, preserving `/` characters.
fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push(hex_char(byte >> 4));
                out.push(hex_char(byte & 0x0F));
            }
        }
    }
    out
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + (nibble - 10)) as char,
    }
}
