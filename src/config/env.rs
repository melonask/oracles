use crate::error::{Error, Result};

/// Expand `${VAR_NAME}` and `${VAR_NAME:-default}` placeholders in a
/// string with environment variable values.
///
/// For `${VAR:-default}`, returns the default if the variable is unset or
/// empty. Unmatched `$` without `{` are left as-is. Returns an error if a
/// placeholder is never closed or if a variable is missing without a default.
pub fn expand_env(input: &str) -> Result<String> {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            output.push(ch);
            continue;
        }

        if chars.peek() != Some(&'{') {
            output.push(ch);
            continue;
        }

        chars.next();

        let mut key = String::new();
        let mut default_value: Option<String> = None;

        loop {
            match chars.next() {
                Some(':') => {
                    // Check if followed by `-` for the `${VAR:-default}` syntax
                    if chars.peek() == Some(&'-') {
                        chars.next(); // consume the `-`
                        default_value = Some(read_until_close(&mut chars)?);
                        break;
                    } else {
                        key.push(':');
                    }
                }
                Some('}') => break,
                Some(c) => key.push(c),
                None => {
                    return Err(Error::Env(
                        "unclosed environment variable placeholder".to_owned(),
                    ));
                }
            }
        }

        if let Some(default) = default_value {
            let value = std::env::var(&key).ok().filter(|v| !v.is_empty());
            output.push_str(value.as_deref().unwrap_or(&default));
        } else {
            let value = std::env::var(&key)
                .map_err(|_| Error::Env(format!("missing environment variable: {key}")))?;
            output.push_str(&value);
        }
    }

    Ok(output)
}

fn read_until_close(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<String> {
    let mut buf = String::new();
    loop {
        match chars.next() {
            Some('}') => break,
            Some(c) => buf.push(c),
            None => {
                return Err(Error::Env(
                    "unclosed environment variable placeholder".to_owned(),
                ));
            }
        }
    }
    Ok(buf)
}
