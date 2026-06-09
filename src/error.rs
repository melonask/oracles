/// Convenience type alias for the crate's [`Result`] type.
pub type Result<T> = core::result::Result<T, Error>;

/// Top-level error type for the `oracles` crate.
#[derive(Debug)]
pub enum Error {
    /// An I/O error occurred (file read, network, etc.).
    Io(std::io::Error),
    /// A configuration validation or parsing error.
    Config(String),
    /// An environment variable lookup or expansion error.
    Env(String),
    /// An invalid asset identifier was encountered.
    InvalidAssetId(String),
    /// An invalid chain identifier was encountered.
    InvalidChainId(String),
    /// An invalid provider identifier was encountered.
    InvalidProviderId(String),
    /// An invalid quote currency (must be ASCII uppercase) was encountered.
    InvalidQuote(String),
    /// A decimal value could not be parsed.
    InvalidDecimal(String),
    /// A rate value was invalid (e.g., zero or negative).
    InvalidRate(String),
    /// A provider fetch or parse error occurred.
    Provider(String),
    /// A store (database) error occurred.
    Store(String),
    /// A safety-engine evaluation error occurred.
    Safety(String),
    /// A template rendering error occurred.
    Template(String),
    /// A JSON path lookup error occurred.
    JsonPath(String),
    /// The user requested `--help` (not a real error).
    HelpRequested,
    /// An unknown CLI argument was passed.
    UnknownArgument(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Config(msg) => write!(f, "config error: {msg}"),
            Self::Env(msg) => write!(f, "environment error: {msg}"),
            Self::InvalidAssetId(v) => write!(f, "invalid asset id: {v}"),
            Self::InvalidChainId(v) => write!(f, "invalid chain id: {v}"),
            Self::InvalidProviderId(v) => write!(f, "invalid provider id: {v}"),
            Self::InvalidQuote(v) => write!(f, "invalid quote: {v}"),
            Self::InvalidDecimal(v) => write!(f, "invalid decimal: {v}"),
            Self::InvalidRate(v) => write!(f, "invalid rate: {v}"),
            Self::Provider(msg) => write!(f, "provider error: {msg}"),
            Self::Store(msg) => write!(f, "store error: {msg}"),
            Self::Safety(msg) => write!(f, "safety error: {msg}"),
            Self::Template(msg) => write!(f, "template error: {msg}"),
            Self::JsonPath(msg) => write!(f, "json path error: {msg}"),
            Self::HelpRequested => f.write_str("help requested"),
            Self::UnknownArgument(arg) => write!(f, "unknown argument: {arg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
