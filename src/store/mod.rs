//! Store abstraction: trait and type aliases.

use crate::domain::{AssetId, EventReason, EventType, OracleEvent, ProviderId, Quote, RateRecord};
use crate::error::Result;
use time::OffsetDateTime;

/// Optional database row ID returned when an event is stored.
pub type EventRowId = Option<i64>;

/// A pending outbox delivery row.
#[derive(Clone, Debug)]
pub struct OutboxDelivery {
    /// Row primary key.
    pub id: i64,
    /// Optional FK to the event that triggered this delivery.
    pub event_id: EventRowId,
    /// The sink identifier (e.g. "telegram", "webhook").
    pub sink: String,
    /// The rendered payload to deliver.
    pub payload: String,
    /// Number of delivery attempts so far.
    pub attempts: u32,
}

/// Outbox store operations for dispatch and retry management.
pub trait OutboxStore {
    /// Load pending deliveries that are due for dispatch.
    fn pending_outbox(&mut self, now: OffsetDateTime, limit: usize) -> Result<Vec<OutboxDelivery>>;

    /// Mark an outbox row as successfully delivered.
    fn mark_outbox_delivered(&mut self, id: i64, delivered_at: OffsetDateTime) -> Result<()>;

    /// Mark an outbox row as failed, optionally scheduling a retry.
    fn mark_outbox_failed(
        &mut self,
        id: i64,
        attempts: u32,
        next_attempt_at: Option<OffsetDateTime>,
        last_error: &str,
    ) -> Result<()>;
}

/// Core store trait for rate persistence and event recording.
pub trait RateStore {
    /// Begin a decision transaction or critical section.
    ///
    /// Default implementation is a no-op for test stores.
    fn begin_decision(&mut self) -> Result<()> {
        Ok(())
    }

    /// Commit a decision transaction or critical section.
    ///
    /// Default implementation is a no-op for test stores.
    fn commit_decision(&mut self) -> Result<()> {
        Ok(())
    }

    /// Roll back a decision transaction or critical section.
    ///
    /// Default implementation is a no-op for test stores.
    fn rollback_decision(&mut self) -> Result<()> {
        Ok(())
    }

    /// Load the latest accepted rate for the given asset and quote.
    ///
    /// Implementations should read from durable state, not process memory.
    fn last_accepted_rate(
        &mut self,
        asset_id: &AssetId,
        quote: &Quote,
    ) -> Result<Option<RateRecord>>;

    /// Load the latest accepted rate for the given asset, quote, and provider.
    ///
    /// This is used in "all" selection mode to compare each candidate against
    /// its own provider's previous rate, rather than the global last accepted rate.
    fn last_accepted_rate_for_provider(
        &mut self,
        asset_id: &AssetId,
        quote: &Quote,
        provider: &ProviderId,
    ) -> Result<Option<RateRecord>> {
        // Default: fall back to last_accepted_rate (not provider-specific)
        let _ = provider;
        self.last_accepted_rate(asset_id, quote)
    }

    /// Write an accepted active rate.
    fn write_accepted_rate(&mut self, record: &RateRecord) -> Result<()>;

    /// Write an audit event. Returns an optional row identifier.
    fn write_event(&mut self, event: &OracleEvent) -> Result<EventRowId>;

    /// Write a pending outbox delivery.
    fn write_outbox(
        &mut self,
        event_id: EventRowId,
        event: &OracleEvent,
        sink: &str,
        payload: &str,
    ) -> Result<()>;

    /// Check whether a recent event of the same type exists for this
    /// asset/provider/reason within the cooldown window. Returns `true` if a
    /// matching event was found.
    fn has_recent_event(
        &mut self,
        asset_id: &AssetId,
        provider: &ProviderId,
        event_type: &EventType,
        reason: &EventReason,
        within_secs: u64,
    ) -> Result<bool> {
        let _ = (asset_id, provider, event_type, reason, within_secs);
        Ok(false) // default: no cooldown tracking
    }

    /// Check whether a `disable_asset` event exists for the given asset.
    ///
    /// Returns `true` if the most recent action for this asset is
    /// `disable_asset`. Implementations should query the events table for
    /// the latest action, ordered by `observed_at DESC, id DESC`, and
    /// check if the returned action equals `"disable_asset"`. This allows
    /// a manual review or re-enable to supersede a previous disable.
    fn has_recent_disable_event(&mut self, asset_id: &AssetId) -> Result<bool> {
        let _ = asset_id;
        Ok(false) // default: no disable tracking
    }

    /// Look up the last observed (fetched) rate for an asset/quote.
    ///
    /// Queries both the rates table (latest accepted rate) and events table
    /// (latest event with `candidate_rate`) and returns whichever has the
    /// most recent `observed_at` timestamp. Uses `stale_after_secs` to
    /// compute `expires_at` for event-sourced records.
    fn last_observed_rate(
        &mut self,
        asset_id: &AssetId,
        quote: &Quote,
        stale_after_secs: u64,
    ) -> Result<Option<RateRecord>> {
        let _ = (asset_id, quote, stale_after_secs);
        Ok(None) // default: no observed rate tracking
    }
}

#[cfg(feature = "sqlite")]
/// SQLite-backed rate and event store.
pub mod sqlite;

#[cfg(feature = "postgres")]
/// PostgreSQL-backed rate and event store.
pub mod postgres;

/// Database schema migration support.
pub mod migration;
