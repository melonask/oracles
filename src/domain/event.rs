use crate::domain::{AssetId, ChainId, ProviderId, Quote, RateAmount};
use rust_decimal::Decimal;
use time::OffsetDateTime;

/// An oracle event produced when a rate triggers a safety action.
///
/// Events carry the full context of what happened: the asset, the provider,
/// the previous and candidate rates, the change percentage, and the action
/// and reason for the event.
#[derive(Clone, Debug)]
pub struct OracleEvent {
    /// The type of event (anomaly, quarantine, reject, etc.).
    pub event_type: EventType,
    /// The affected asset identifier.
    pub asset_id: AssetId,
    /// The chain identifier (if applicable).
    pub chain_id: Option<ChainId>,
    /// The asset symbol.
    pub symbol: String,
    /// The quote currency.
    pub quote: Quote,
    /// The provider that triggered the event.
    pub provider: ProviderId,
    /// The previously accepted rate (if any).
    pub previous_rate: Option<RateAmount>,
    /// The candidate rate that triggered the event.
    pub candidate_rate: Option<RateAmount>,
    /// The percent change from the previous rate (if computed).
    pub change_pct: Option<Decimal>,
    /// The action taken in response to this event.
    pub action: EventAction,
    /// The reason this event was triggered.
    pub reason: EventReason,
    /// When the source data was last updated (if known).
    pub source_updated_at: Option<OffsetDateTime>,
    /// When this event was observed.
    pub observed_at: OffsetDateTime,
}

/// The category of an oracle event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventType {
    /// A rate anomaly was detected (bound, spike, stale source).
    RateAnomaly,
    /// A rate was quarantined (accepted but flagged).
    RateQuarantined,
    /// A rate was rejected outright.
    RateRejected,
    /// A provider failed to return a valid rate.
    ProviderFailed,
    /// An entire refresh cycle failed for an asset.
    RefreshFailed,
}

impl EventType {
    /// Return the event type as a dot-separated string (e.g., `"oracle.rate_anomaly"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RateAnomaly => "oracle.rate_anomaly",
            Self::RateQuarantined => "oracle.rate_quarantined",
            Self::RateRejected => "oracle.rate_rejected",
            Self::ProviderFailed => "oracle.provider_failed",
            Self::RefreshFailed => "oracle.refresh_failed",
        }
    }
}

/// The action taken in response to an oracle event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventAction {
    /// Log an alert but still accept the rate.
    Alert,
    /// Accept the rate but flag it for review.
    Quarantine,
    /// Reject the rate entirely.
    Reject,
    /// Disable the asset so it is no longer refreshed.
    DisableAsset,
}

impl EventAction {
    /// Return the action as a snake_case string (e.g., `"disable_asset"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Alert => "alert",
            Self::Quarantine => "quarantine",
            Self::Reject => "reject",
            Self::DisableAsset => "disable_asset",
        }
    }

    /// Return the event type emitted when this action is selected.
    pub fn event_type(&self) -> EventType {
        match self {
            Self::Alert => EventType::RateAnomaly,
            Self::Quarantine => EventType::RateQuarantined,
            Self::Reject | Self::DisableAsset => EventType::RateRejected,
        }
    }
}

/// The reason an oracle event was triggered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventReason {
    /// The percent change from the previous rate exceeded the configured maximum.
    MaxChangeExceeded,
    /// The spread across providers exceeded the configured maximum.
    ProviderSpreadExceeded,
    /// The source data timestamp was too old.
    SourceTimestampTooOld,
    /// The rate was below the configured minimum.
    RateBelowMin,
    /// The rate was above the configured maximum.
    RateAboveMax,
    /// No previous rate was available to compare against.
    MissingPreviousRate,
    /// The provider returned an error.
    ProviderError,
    /// The provider response could not be parsed.
    ParseError,
}

impl EventReason {
    /// Return the reason as a snake_case string (e.g., `"max_change_exceeded"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MaxChangeExceeded => "max_change_exceeded",
            Self::ProviderSpreadExceeded => "provider_spread_exceeded",
            Self::SourceTimestampTooOld => "source_timestamp_too_old",
            Self::RateBelowMin => "rate_below_min",
            Self::RateAboveMax => "rate_above_max",
            Self::MissingPreviousRate => "missing_previous_rate",
            Self::ProviderError => "provider_error",
            Self::ParseError => "parse_error",
        }
    }
}
