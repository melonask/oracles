use crate::domain::{OracleEvent, RateRecord};

/// The outcome of a safety evaluation for a candidate rate.
///
/// After the safety engine inspects a [`CandidateRate`], it produces one of
/// these decisions. The caller is responsible for persisting the accepted
/// rate or recording the event accordingly.
#[derive(Clone, Debug)]
pub enum Decision {
    /// The rate passed all checks and is accepted.
    Accept(RateRecord),
    /// The rate triggered a safety rule but is still accepted with an alert.
    Alert {
        /// The accepted rate record.
        record: RateRecord,
        /// The event describing what triggered the alert.
        event: Box<OracleEvent>,
    },
    /// The rate was quarantined (flagged for review).
    Quarantine(OracleEvent),
    /// The rate was rejected outright.
    Reject(OracleEvent),
    /// The entire asset was disabled.
    DisableAsset(OracleEvent),
}
