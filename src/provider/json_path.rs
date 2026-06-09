#[cfg(feature = "http-json")]
use serde_json::Value;

/// Traverse a JSON value using a dot-separated path.
///
/// For example, `"ethereum.usd"` on `{"ethereum": {"usd": 3500}}` returns
/// the inner `3500` value. Returns `None` if any segment is not found.
#[cfg(feature = "http-json")]
pub fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;

    for part in path.split('.') {
        current = current.get(part)?;
    }

    Some(current)
}
