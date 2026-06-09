use crate::config::raw::RawConfig;
use crate::config::resolved::ResolvedConfig;
use crate::config::validate::resolve_config;
use crate::error::{Error, Result};

#[cfg(feature = "config-toml")]
/// Load and resolve a configuration from a TOML file path.
///
/// Reads the file, deserializes it into a [`RawConfig`], and runs the full
/// validation pipeline via [`resolve_config`].
pub fn load_config(path: impl AsRef<std::path::Path>) -> Result<ResolvedConfig> {
    let text = std::fs::read_to_string(path)?;
    let raw: RawConfig = toml::from_str(&text)
        .map_err(|err| Error::Config(format!("failed to parse TOML: {err}")))?;

    resolve_config(raw)
}

#[cfg(not(feature = "config-toml"))]
/// Stub that returns an error when the `config-toml` feature is disabled.
pub fn load_config(_path: impl AsRef<std::path::Path>) -> Result<ResolvedConfig> {
    Err(Error::Config("config-toml feature is disabled".to_owned()))
}
