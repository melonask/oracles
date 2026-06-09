use crate::error::{Error, Result};

/// A validated asset identifier (e.g., `"eth"`, `"usdc_base"`).
///
/// Asset IDs must be non-empty, lowercase, and contain only ASCII letters,
/// digits, underscores, and hyphens.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(String);

/// A validated chain identifier (e.g., `"eth"`, `"polygon"`).
///
/// Chain IDs follow the same rules as [`AssetId`]: non-empty, lowercase
/// ASCII alphanumeric with underscores and hyphens.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChainId(String);

/// A validated provider identifier (e.g., `"coingecko"`, `"diadata"`).
///
/// Provider IDs follow the same rules as [`AssetId`]: non-empty, lowercase
/// ASCII alphanumeric with underscores and hyphens.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(String);

/// A validated quote currency (e.g., `"USD"`, `"EUR"`).
///
/// Quote currencies must be non-empty and contain only ASCII uppercase
/// letters.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quote(String);

impl AssetId {
    /// Create a new [`AssetId`] from a string-like value.
    ///
    /// Returns an error if the value does not match the required format
    /// (lowercase ASCII alphanumeric, underscores, hyphens).
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if is_lower_snake_id(&value) {
            Ok(Self(value))
        } else {
            Err(Error::InvalidAssetId(value))
        }
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ChainId {
    /// Create a new [`ChainId`] from a string-like value.
    ///
    /// Returns an error if the value does not match the required format.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if is_lower_snake_id(&value) {
            Ok(Self(value))
        } else {
            Err(Error::InvalidChainId(value))
        }
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ProviderId {
    /// Create a new [`ProviderId`] from a string-like value.
    ///
    /// Returns an error if the value does not match the required format.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if is_lower_snake_id(&value) {
            Ok(Self(value))
        } else {
            Err(Error::InvalidProviderId(value))
        }
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Quote {
    /// Create a new [`Quote`] from a string-like value.
    ///
    /// Returns an error if the value is empty or contains non-uppercase
    /// ASCII characters.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if !value.is_empty() && value.bytes().all(|b| b.is_ascii_uppercase()) {
            Ok(Self(value))
        } else {
            Err(Error::InvalidQuote(format!(
                "quote must be non-empty ASCII uppercase (e.g., \"USD\"), got: {value}"
            )))
        }
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return the quote currency in lowercase (e.g., `"usd"` for `"USD"`).
    pub fn lower(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

#[allow(missing_docs)]
impl core::fmt::Display for AssetId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[allow(missing_docs)]
impl core::fmt::Display for ChainId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[allow(missing_docs)]
impl core::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[allow(missing_docs)]
impl core::fmt::Display for Quote {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn is_lower_snake_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[allow(missing_docs)]
impl PartialEq<&str> for AssetId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[allow(missing_docs)]
impl PartialEq<&str> for ChainId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[allow(missing_docs)]
impl PartialEq<&str> for ProviderId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[allow(missing_docs)]
impl PartialEq<&str> for Quote {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}
