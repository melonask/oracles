//! Outbox dispatcher for reliable notification delivery.

use crate::config::ResolvedConfig;
use crate::error::{Error, Result};
use crate::events::sinks::build_sink;
use crate::store::OutboxStore;
use time::{Duration, OffsetDateTime};

/// Dispatches pending outbox deliveries to their configured sinks.
pub struct OutboxDispatcher<'a, S> {
    config: &'a ResolvedConfig,
    store: S,
}

impl<'a, S> OutboxDispatcher<'a, S>
where
    S: OutboxStore,
{
    /// Create a new dispatcher wrapping the given store.
    pub fn new(config: &'a ResolvedConfig, store: S) -> Self {
        Self { config, store }
    }

    /// Process up to `limit` pending deliveries.
    ///
    /// Each delivery is attempted once. Successful deliveries are marked
    /// as delivered. Failed deliveries are retried or moved to dead status.
    ///
    /// Store update errors are collected and returned after all due deliveries
    /// have been attempted. This keeps one broken row from blocking the rest
    /// of the batch while still surfacing durability problems to callers.
    pub fn dispatch_once(&mut self, limit: usize) -> Result<DispatchSummary> {
        let now = OffsetDateTime::now_utc();
        let deliveries = self.store.pending_outbox(now, limit)?;
        let mut summary = DispatchSummary::default();

        if deliveries.is_empty() {
            return Ok(summary);
        }

        let max_retries = self.config.outbox.max_retries;
        let retry_backoff = self.config.outbox.retry_backoff_secs;
        let mut store_errors: Vec<String> = Vec::new();

        for delivery in deliveries {
            summary.attempted += 1;

            let sink = match self
                .config
                .events
                .sinks
                .get(&delivery.sink)
                .ok_or_else(|| {
                    Error::Config(format!("unknown event sink in outbox: {}", delivery.sink))
                })
                .and_then(build_sink)
            {
                Ok(sink) => sink,
                Err(err) => {
                    crate::error!("failed to build sink for outbox delivery: {err}");
                    let new_attempts = delivery.attempts.saturating_add(1);

                    if let Err(store_err) = self.store.mark_outbox_failed(
                        delivery.id,
                        new_attempts,
                        None,
                        &err.to_string(),
                    ) {
                        store_errors.push(format!(
                            "failed to mark outbox {} as dead: {store_err}",
                            delivery.id
                        ));
                    }

                    summary.dead += 1;
                    continue;
                }
            };

            match sink.deliver(&delivery.payload) {
                Ok(()) => {
                    if let Err(store_err) = self
                        .store
                        .mark_outbox_delivered(delivery.id, OffsetDateTime::now_utc())
                    {
                        store_errors.push(format!(
                            "failed to mark outbox {} as delivered: {store_err}",
                            delivery.id
                        ));
                    }
                    summary.delivered += 1;
                }
                Err(err) => {
                    let new_attempts = delivery.attempts.saturating_add(1);

                    if new_attempts >= max_retries {
                        crate::error!("outbox delivery dead for id={}: {err}", delivery.id);
                        if let Err(store_err) = self.store.mark_outbox_failed(
                            delivery.id,
                            new_attempts,
                            None,
                            &err.to_string(),
                        ) {
                            store_errors.push(format!(
                                "failed to mark outbox {} as dead: {store_err}",
                                delivery.id
                            ));
                        }
                        summary.dead += 1;
                    } else {
                        let next_attempt_at =
                            OffsetDateTime::now_utc() + Duration::seconds(retry_backoff as i64);

                        if let Err(store_err) = self.store.mark_outbox_failed(
                            delivery.id,
                            new_attempts,
                            Some(next_attempt_at),
                            &err.to_string(),
                        ) {
                            store_errors.push(format!(
                                "failed to mark outbox {} as failed: {store_err}",
                                delivery.id
                            ));
                        }
                        summary.failed += 1;
                    }
                }
            }
        }

        if !store_errors.is_empty() {
            return Err(Error::Store(format!(
                "outbox dispatch completed with {} store error(s): {}",
                store_errors.len(),
                store_errors.join("; ")
            )));
        }

        Ok(summary)
    }

    /// Consume the dispatcher and return the inner store.
    pub fn into_store(self) -> S {
        self.store
    }
}

/// Summary of a single dispatch pass.
#[derive(Clone, Debug, Default)]
pub struct DispatchSummary {
    /// Total deliveries attempted.
    pub attempted: usize,
    /// Deliveries that succeeded.
    pub delivered: usize,
    /// Deliveries that failed but will be retried.
    pub failed: usize,
    /// Deliveries that exceeded max retries and are now dead.
    pub dead: usize,
}
