use crate::config::{
    BootstrapAction, CompareAgainst, ResolvedAsset, ResolvedConfig, ResolvedFeed, ResolvedProvider,
    SelectionMode,
};
use crate::domain::{
    CandidateRate, Decision, EventAction, EventReason, EventType, OracleEvent, ProviderId,
    RateRecord,
};
use crate::error::{Error, Result};
use crate::provider::{Provider, ProviderContext};
use crate::safety::SafetyEngine;
use crate::store::RateStore;
use std::sync::Arc;
use time::OffsetDateTime;

/// Map an [`EventAction`] to the corresponding [`EventType`].
fn event_type_for_action(action: &EventAction) -> EventType {
    match action {
        EventAction::Alert => EventType::RateAnomaly,
        EventAction::Quarantine => EventType::RateQuarantined,
        EventAction::Reject => EventType::RateRejected,
        EventAction::DisableAsset => EventType::RateRejected,
    }
}

/// The core oracle engine that orchestrates rate fetching, safety evaluation,
/// and persistence.
///
/// `S` is the store backend (must implement [`RateStore`]).
pub struct Oracle<S> {
    config: ResolvedConfig,
    store: S,
    safety: SafetyEngine,
    providers: Vec<Arc<dyn Provider>>,
}

impl<S> Oracle<S>
where
    S: RateStore,
{
    /// Create a new [`Oracle`] instance.
    ///
    /// Takes a resolved configuration, a store backend, and a list of
    /// provider implementations.
    pub fn new(config: ResolvedConfig, store: S, providers: Vec<Arc<dyn Provider>>) -> Self {
        let safety = SafetyEngine::new(config.safety.clone());
        Self {
            config,
            store,
            safety,
            providers,
        }
    }

    /// Run a single refresh cycle for all enabled assets.
    ///
    /// Fetches rates from providers, evaluates them through the safety
    /// engine, and persists the results. Returns a [`RefreshSummary`]
    /// with attempt/success/failure counts.
    pub fn run_once(&mut self) -> Result<RefreshSummary> {
        let mut summary = RefreshSummary::default();
        let assets = self.config.enabled_assets();
        for asset in assets {
            let result = self.refresh_asset(&asset);
            summary.record(&asset.id, result.is_ok());
            if result.is_err() && self.config.oracles.fail_fast {
                return result.map(|_| summary);
            }
        }
        Ok(summary)
    }

    /// Return a reference to the resolved configuration.
    pub fn config(&self) -> &ResolvedConfig {
        &self.config
    }

    /// Consume the Oracle and return the store.
    pub fn into_store(self) -> S {
        self.store
    }

    /// Return a mutable reference to the inner store.
    ///
    /// This allows external dispatch loops to access outbox operations
    /// without taking ownership.
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Dispatch pending outbox deliveries.
    ///
    /// Shared helper used by both `--once` and the continuous `run_loop`.
    /// Processes up to `limit` pending deliveries. Returns a summary of
    /// attempted/delivered/failed/dead counts.
    pub fn dispatch_outbox(&mut self, limit: usize) -> Result<DispatchOutboxSummary>
    where
        S: crate::store::OutboxStore,
    {
        let mut summary = DispatchOutboxSummary::default();
        let now = time::OffsetDateTime::now_utc();
        let deliveries = self.store.pending_outbox(now, limit)?;

        if deliveries.is_empty() {
            return Ok(summary);
        }

        crate::info!("dispatching {} pending outbox deliveries", deliveries.len());
        let sinks = self.config.events.sinks.clone();
        let max_retries = self.config.outbox.max_retries;
        let retry_backoff = self.config.outbox.retry_backoff_secs;

        // Collect store update errors so we can report them after
        // processing all deliveries.
        let mut store_errors: Vec<String> = Vec::new();

        for delivery in deliveries {
            summary.attempted += 1;
            let sink_config = sinks.get(&delivery.sink);
            let sink = match sink_config
                .ok_or_else(|| Error::Config(format!("unknown sink in outbox: {}", delivery.sink)))
                .and_then(|cfg| crate::events::sinks::build_sink(cfg))
            {
                Ok(sink) => sink,
                Err(err) => {
                    crate::error!("failed to build sink for outbox delivery: {err}");
                    if let Err(e) = self.store.mark_outbox_failed(
                        delivery.id,
                        delivery.attempts.saturating_add(1),
                        None,
                        &err.to_string(),
                    ) {
                        store_errors.push(format!(
                            "failed to mark outbox {} as failed: {e}",
                            delivery.id
                        ));
                    }
                    summary.dead += 1;
                    continue;
                }
            };

            match sink.deliver(&delivery.payload) {
                Ok(()) => {
                    if let Err(e) = self
                        .store
                        .mark_outbox_delivered(delivery.id, time::OffsetDateTime::now_utc())
                    {
                        store_errors.push(format!(
                            "failed to mark outbox {} as delivered: {e}",
                            delivery.id
                        ));
                    }
                    summary.delivered += 1;
                }
                Err(err) => {
                    let new_attempts = delivery.attempts.saturating_add(1);
                    if new_attempts >= max_retries {
                        crate::error!("outbox delivery dead for id={}: {err}", delivery.id);
                        if let Err(e) = self.store.mark_outbox_failed(
                            delivery.id,
                            new_attempts,
                            None,
                            &err.to_string(),
                        ) {
                            store_errors.push(format!(
                                "failed to mark outbox {} as dead: {e}",
                                delivery.id
                            ));
                        }
                        summary.dead += 1;
                    } else {
                        let next = time::OffsetDateTime::now_utc()
                            + time::Duration::seconds(retry_backoff as i64);
                        if let Err(e) = self.store.mark_outbox_failed(
                            delivery.id,
                            new_attempts,
                            Some(next),
                            &err.to_string(),
                        ) {
                            store_errors.push(format!(
                                "failed to mark outbox {} as failed: {e}",
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

    /// Get the previous rate for comparison based on the configured
    /// [`CompareAgainst`] mode.
    fn get_previous_rate(
        &mut self,
        asset_id: &crate::domain::AssetId,
    ) -> Result<Option<crate::domain::RateRecord>> {
        let quote = &self.config.oracles.quote;
        match self.config.safety.compare_against {
            CompareAgainst::LastAccepted => self.store.last_accepted_rate(asset_id, quote),
            CompareAgainst::LastObserved => {
                self.store
                    .last_observed_rate(asset_id, quote, self.config.oracles.stale_after_secs)
            }
        }
    }

    /// Check if an asset has been disabled by a previous `disable_asset`
    /// event.
    ///
    /// Returns `true` if the most recent relevant event for this asset has
    /// `action = "disable_asset"`.
    fn is_asset_disabled(&mut self, asset_id: &crate::domain::AssetId) -> Result<bool> {
        if !self.config.events.enabled || !self.config.events.record {
            return Ok(false);
        }
        self.store.has_recent_disable_event(asset_id)
    }

    /// Dispatch to the correct refresh strategy based on selection mode.
    fn refresh_asset(&mut self, asset: &ResolvedAsset) -> Result<()> {
        match self.config.oracles.selection {
            SelectionMode::Priority => self.refresh_asset_priority(asset),
            SelectionMode::All => self.refresh_asset_all(asset),
            SelectionMode::Median => self.refresh_asset_median(asset),
        }
    }

    /// Priority mode: try feeds in priority order, use the first success.
    fn refresh_asset_priority(&mut self, asset: &ResolvedAsset) -> Result<()> {
        // Check whether this asset has been disabled by a previous event.
        if self.is_asset_disabled(&asset.id)? {
            crate::info!(
                "asset `{}` is disabled by a previous disable_asset event, skipping",
                asset.id.as_str()
            );
            return Ok(());
        }

        // Fetch outside the DB transaction. Do not hold a SQLite write
        // transaction during HTTP calls.
        let candidate = match self.fetch_candidate(asset) {
            Ok(c) => c,
            Err(err) => {
                self.record_refresh_failed(asset, &err)?;
                return Err(err);
            }
        };

        self.store.begin_decision()?;
        let result = (|| {
            let previous = self.get_previous_rate(&asset.id)?;
            let safety_enabled = self.config.safety.enabled && asset.safety_enabled;
            let decision = if safety_enabled {
                // Enforce RequireMultipleProviders bootstrap: priority mode
                // only yields one candidate, so multi-provider cannot be
                // satisfied.
                if previous.is_none()
                    && self.config.safety.bootstrap_action
                        == BootstrapAction::RequireMultipleProviders
                {
                    let action = self.config.safety.default_action.clone();
                    let event_type = event_type_for_action(&action);
                    let event = OracleEvent {
                        event_type,
                        asset_id: candidate.asset_id.clone(),
                        chain_id: Some(candidate.chain_id.clone()),
                        symbol: candidate.symbol.clone(),
                        quote: candidate.quote.clone(),
                        provider: candidate.provider.clone(),
                        previous_rate: None,
                        candidate_rate: Some(candidate.rate.clone()),
                        change_pct: None,
                        action: action.clone(),
                        reason: EventReason::MissingPreviousRate,
                        source_updated_at: candidate.source_updated_at,
                        observed_at: candidate.observed_at,
                    };
                    match action {
                        EventAction::Alert => {
                            let expires_at = candidate.observed_at + self.config.safety.stale_after;
                            Decision::Alert {
                                record: RateRecord {
                                    asset_id: candidate.asset_id.clone(),
                                    chain_id: candidate.chain_id.clone(),
                                    caip2: candidate.caip2.clone(),
                                    symbol: candidate.symbol.clone(),
                                    quote: candidate.quote.clone(),
                                    provider: candidate.provider.clone(),
                                    rate: candidate.rate.clone(),
                                    source_updated_at: candidate.source_updated_at,
                                    observed_at: candidate.observed_at,
                                    expires_at,
                                },
                                event: Box::new(event),
                            }
                        }
                        EventAction::Quarantine => Decision::Quarantine(event),
                        EventAction::Reject => Decision::Reject(event),
                        EventAction::DisableAsset => Decision::DisableAsset(event),
                    }
                } else {
                    self.safety.evaluate(
                        asset,
                        previous.as_ref(),
                        candidate,
                        OffsetDateTime::now_utc(),
                    )?
                }
            } else {
                // Safety disabled: accept the candidate unconditionally.
                let expires_at = candidate.observed_at + self.config.safety.stale_after;
                Decision::Accept(RateRecord {
                    asset_id: candidate.asset_id,
                    chain_id: candidate.chain_id,
                    caip2: candidate.caip2,
                    symbol: candidate.symbol,
                    quote: candidate.quote,
                    provider: candidate.provider,
                    rate: candidate.rate,
                    source_updated_at: candidate.source_updated_at,
                    observed_at: candidate.observed_at,
                    expires_at,
                })
            };
            self.apply_decision(decision)
        })();
        match result {
            Ok(()) => {
                self.store.commit_decision()?;
                Ok(())
            }
            Err(err) => {
                let _ = self.store.rollback_decision();
                Err(err)
            }
        }
    }

    /// All mode: fetch every enabled feed, evaluate each candidate individually.
    ///
    /// Runs consensus checking first. If consensus triggers an action, that
    /// action is applied and individual evaluation is skipped.
    fn refresh_asset_all(&mut self, asset: &ResolvedAsset) -> Result<()> {
        // Check whether this asset has been disabled by a previous event.
        if self.is_asset_disabled(&asset.id)? {
            crate::info!(
                "asset `{}` is disabled by a previous disable_asset event, skipping",
                asset.id.as_str()
            );
            return Ok(());
        }

        let candidates = match self.fetch_all_candidates(asset) {
            Ok(c) => c,
            Err(err) => {
                self.record_refresh_failed(asset, &err)?;
                return Err(err);
            }
        };

        self.store.begin_decision()?;
        let result = (|| {
            let previous = self.get_previous_rate(&asset.id)?;
            let safety_enabled = self.config.safety.enabled && asset.safety_enabled;

            // Enforce RequireMultipleProviders bootstrap: require at least 2
            // successful candidates when no previous rate exists.
            if safety_enabled
                && previous.is_none()
                && self.config.safety.bootstrap_action == BootstrapAction::RequireMultipleProviders
                && candidates.len() < 2
            {
                let Some(first) = candidates.first() else {
                    return Err(Error::Provider(format!(
                        "RequireMultipleProviders: no candidates available for asset: {}",
                        asset.id.as_str()
                    )));
                };
                let action = self.config.safety.default_action.clone();
                let event_type = event_type_for_action(&action);
                let event = OracleEvent {
                    event_type,
                    asset_id: first.asset_id.clone(),
                    chain_id: Some(first.chain_id.clone()),
                    symbol: first.symbol.clone(),
                    quote: first.quote.clone(),
                    provider: first.provider.clone(),
                    previous_rate: None,
                    candidate_rate: Some(first.rate.clone()),
                    change_pct: None,
                    action: action.clone(),
                    reason: EventReason::MissingPreviousRate,
                    source_updated_at: first.source_updated_at,
                    observed_at: first.observed_at,
                };
                let decision = match action {
                    EventAction::Alert => {
                        let expires_at = first.observed_at + self.config.safety.stale_after;
                        Decision::Alert {
                            record: RateRecord {
                                asset_id: first.asset_id.clone(),
                                chain_id: first.chain_id.clone(),
                                caip2: first.caip2.clone(),
                                symbol: first.symbol.clone(),
                                quote: first.quote.clone(),
                                provider: first.provider.clone(),
                                rate: first.rate.clone(),
                                source_updated_at: first.source_updated_at,
                                observed_at: first.observed_at,
                                expires_at,
                            },
                            event: Box::new(event),
                        }
                    }
                    EventAction::Quarantine => Decision::Quarantine(event),
                    EventAction::Reject => Decision::Reject(event),
                    EventAction::DisableAsset => Decision::DisableAsset(event),
                };
                return self.apply_decision(decision);
            }

            if !safety_enabled {
                // Safety disabled: accept all candidates unconditionally.
                let mut any_success = false;
                let mut last_error: Option<Error> = None;
                for candidate in candidates {
                    let expires_at = candidate.observed_at + self.config.safety.stale_after;
                    let decision = Decision::Accept(RateRecord {
                        asset_id: candidate.asset_id,
                        chain_id: candidate.chain_id,
                        caip2: candidate.caip2,
                        symbol: candidate.symbol,
                        quote: candidate.quote,
                        provider: candidate.provider,
                        rate: candidate.rate,
                        source_updated_at: candidate.source_updated_at,
                        observed_at: candidate.observed_at,
                        expires_at,
                    });
                    match self.apply_decision(decision) {
                        Ok(()) => {
                            any_success = true;
                        }
                        Err(err) => {
                            last_error = Some(err);
                        }
                    }
                }
                return if any_success {
                    Ok(())
                } else {
                    Err(last_error.unwrap_or_else(|| {
                        Error::Provider(format!(
                            "all candidates failed to write for asset: {}",
                            asset.id.as_str()
                        ))
                    }))
                };
            }

            // Consensus check first
            if let Some(decision) =
                self.safety
                    .check_consensus(asset, &candidates, previous.as_ref())?
            {
                return self.apply_decision(decision);
            }

            // Consensus passed: evaluate each candidate against its own
            // per-provider previous rate in All mode.
            let mut any_success = false;
            let mut last_error: Option<Error> = None;

            for candidate in candidates {
                // In All mode, compare against the per-provider previous rate.
                // When compare_against = LastAccepted, use the provider-specific
                // accepted rate. When compare_against = LastObserved, use the
                // global latest observed rate (which queries both rates and
                // events tables).
                let provider_previous = match self.config.safety.compare_against {
                    CompareAgainst::LastAccepted => self.store.last_accepted_rate_for_provider(
                        &asset.id,
                        &self.config.oracles.quote,
                        &candidate.provider,
                    )?,
                    CompareAgainst::LastObserved => self.store.last_observed_rate(
                        &asset.id,
                        &self.config.oracles.quote,
                        self.config.oracles.stale_after_secs,
                    )?,
                };

                let prev_ref = provider_previous.as_ref().or(previous.as_ref());

                match self
                    .safety
                    .evaluate(asset, prev_ref, candidate, OffsetDateTime::now_utc())
                {
                    Ok(decision) => match self.apply_decision(decision) {
                        Ok(()) => {
                            any_success = true;
                        }
                        Err(err) => {
                            last_error = Some(err);
                        }
                    },
                    Err(err) => {
                        last_error = Some(err);
                    }
                }
            }

            if any_success {
                Ok(())
            } else {
                Err(last_error.unwrap_or_else(|| {
                    Error::Provider(format!(
                        "all candidates failed for asset: {}",
                        asset.id.as_str()
                    ))
                }))
            }
        })();

        match result {
            Ok(()) => {
                self.store.commit_decision()?;
                Ok(())
            }
            Err(err) => {
                let _ = self.store.rollback_decision();
                Err(err)
            }
        }
    }

    /// Median mode: fetch all feeds, compute the median rate, evaluate it.
    ///
    /// Runs consensus checking first. If consensus triggers an action, that
    /// action is applied and the median is not computed.
    fn refresh_asset_median(&mut self, asset: &ResolvedAsset) -> Result<()> {
        // Check whether this asset has been disabled by a previous event.
        if self.is_asset_disabled(&asset.id)? {
            crate::info!(
                "asset `{}` is disabled by a previous disable_asset event, skipping",
                asset.id.as_str()
            );
            return Ok(());
        }

        let candidates = match self.fetch_all_candidates(asset) {
            Ok(c) => c,
            Err(err) => {
                self.record_refresh_failed(asset, &err)?;
                return Err(err);
            }
        };

        self.store.begin_decision()?;
        let result = (|| {
            let previous = self.get_previous_rate(&asset.id)?;
            let safety_enabled = self.config.safety.enabled && asset.safety_enabled;

            // Enforce RequireMultipleProviders bootstrap: require at least 2
            // successful candidates when no previous rate exists.
            if safety_enabled
                && previous.is_none()
                && self.config.safety.bootstrap_action == BootstrapAction::RequireMultipleProviders
                && candidates.len() < 2
            {
                let Some(first) = candidates.first() else {
                    return Err(Error::Provider(format!(
                        "RequireMultipleProviders: no candidates available for asset: {}",
                        asset.id.as_str()
                    )));
                };
                let action = self.config.safety.default_action.clone();
                let event_type = event_type_for_action(&action);
                let event = OracleEvent {
                    event_type,
                    asset_id: first.asset_id.clone(),
                    chain_id: Some(first.chain_id.clone()),
                    symbol: first.symbol.clone(),
                    quote: first.quote.clone(),
                    provider: first.provider.clone(),
                    previous_rate: None,
                    candidate_rate: Some(first.rate.clone()),
                    change_pct: None,
                    action: action.clone(),
                    reason: EventReason::MissingPreviousRate,
                    source_updated_at: first.source_updated_at,
                    observed_at: first.observed_at,
                };
                let decision = match action {
                    EventAction::Alert => {
                        let expires_at = first.observed_at + self.config.safety.stale_after;
                        Decision::Alert {
                            record: RateRecord {
                                asset_id: first.asset_id.clone(),
                                chain_id: first.chain_id.clone(),
                                caip2: first.caip2.clone(),
                                symbol: first.symbol.clone(),
                                quote: first.quote.clone(),
                                provider: first.provider.clone(),
                                rate: first.rate.clone(),
                                source_updated_at: first.source_updated_at,
                                observed_at: first.observed_at,
                                expires_at,
                            },
                            event: Box::new(event),
                        }
                    }
                    EventAction::Quarantine => Decision::Quarantine(event),
                    EventAction::Reject => Decision::Reject(event),
                    EventAction::DisableAsset => Decision::DisableAsset(event),
                };
                return self.apply_decision(decision);
            }

            if !safety_enabled {
                // Safety disabled: accept the median candidate unconditionally.
                let median = Self::select_median_candidate(&candidates);
                let expires_at = median.observed_at + self.config.safety.stale_after;
                let decision = Decision::Accept(RateRecord {
                    asset_id: median.asset_id.clone(),
                    chain_id: median.chain_id.clone(),
                    caip2: median.caip2.clone(),
                    symbol: median.symbol.clone(),
                    quote: median.quote.clone(),
                    provider: median.provider.clone(),
                    rate: median.rate.clone(),
                    source_updated_at: median.source_updated_at,
                    observed_at: median.observed_at,
                    expires_at,
                });
                return self.apply_decision(decision);
            }

            // Consensus check first
            if let Some(decision) =
                self.safety
                    .check_consensus(asset, &candidates, previous.as_ref())?
            {
                return self.apply_decision(decision);
            }

            // Select the median candidate
            let median = Self::select_median_candidate(&candidates);

            // Evaluate the median candidate
            let decision = self.safety.evaluate(
                asset,
                previous.as_ref(),
                median.clone(),
                OffsetDateTime::now_utc(),
            )?;

            self.apply_decision(decision)
        })();

        match result {
            Ok(()) => {
                self.store.commit_decision()?;
                Ok(())
            }
            Err(err) => {
                let _ = self.store.rollback_decision();
                Err(err)
            }
        }
    }

    /// Try feeds in priority order; return the first successful candidate.
    /// Records provider failures as audit events.
    fn fetch_candidate(&mut self, asset: &ResolvedAsset) -> Result<CandidateRate> {
        let mut feeds: Vec<&ResolvedFeed> = asset.enabled_feeds();
        feeds.sort_by_key(|f| std::cmp::Reverse(f.priority));
        let mut last_error = None;
        for feed in feeds {
            match self.fetch_single_candidate(asset, feed) {
                Ok(candidate) => return Ok(candidate),
                Err(err) => {
                    self.record_provider_failure(asset, &feed.provider, &err)?;
                    last_error = Some(err);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            Error::Provider(format!("no enabled feeds for asset: {}", asset.id.as_str()))
        }))
    }

    /// Fetch from every enabled feed; collect all successes, record failures.
    ///
    /// Returns an error if fewer than [`min_successful_feeds`] candidates
    /// were obtained. When `max_concurrent_requests > 1`, feeds are fetched
    /// concurrently using OS threads with bounded concurrency.
    ///
    /// [`min_successful_feeds`]: crate::config::ResolvedConsensusConfig::min_successful_feeds
    fn fetch_all_candidates(&mut self, asset: &ResolvedAsset) -> Result<Vec<CandidateRate>> {
        let feeds: Vec<&ResolvedFeed> = asset.enabled_feeds();
        if feeds.is_empty() {
            return Err(Error::Provider(format!(
                "no enabled feeds for asset: {}",
                asset.id.as_str()
            )));
        }

        let max_concurrent = self.config.oracles.max_concurrent_requests;

        // Sort feeds by priority (descending) for deterministic ordering.
        let mut sorted_feeds: Vec<&ResolvedFeed> = feeds;
        sorted_feeds.sort_by_key(|f| std::cmp::Reverse(f.priority));

        let mut candidates = Vec::new();
        let mut provider_failures: Vec<(ProviderId, Error)> = Vec::new();

        if max_concurrent <= 1 {
            // Sequential path (original behaviour).
            for feed in &sorted_feeds {
                match self.fetch_single_candidate(asset, feed) {
                    Ok(candidate) => candidates.push(candidate),
                    Err(err) => {
                        provider_failures.push((feed.provider.clone(), err));
                    }
                }
            }
        } else {
            // Concurrent path with bounded concurrency using OS threads.
            let observed_at = OffsetDateTime::now_utc();
            let ctx = ProviderContext {
                quote: self.config.oracles.quote.clone(),
                observed_at,
                user_agent: self.config.http.user_agent.clone(),
                request_timeout_secs: self.config.http.request_timeout_secs,
                max_retries: self.config.http.max_retries,
                retry_backoff_ms: self.config.http.retry_backoff_ms,
            };

            // Collect (index, provider_arc, feed, provider_config) tuples.
            let fetch_items: Vec<(usize, Arc<dyn Provider>, ResolvedFeed, ResolvedProvider)> =
                sorted_feeds
                    .iter()
                    .enumerate()
                    .filter_map(|(i, feed)| {
                        let provider = self.providers.iter().find(|p| p.id() == &feed.provider)?;
                        let config = self.config.provider(&feed.provider)?;
                        Some((i, Arc::clone(provider), (*feed).clone(), config.clone()))
                    })
                    .collect();

            let results: std::sync::Mutex<Vec<(usize, Result<CandidateRate>)>> =
                std::sync::Mutex::new(Vec::new());
            let active = std::sync::atomic::AtomicUsize::new(0);

            std::thread::scope(|scope| {
                for (idx, provider, feed, provider_config) in &fetch_items {
                    // Wait for a slot (bounded concurrency via spin-wait).
                    while active.load(std::sync::atomic::Ordering::Acquire) >= max_concurrent {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    active.fetch_add(1, std::sync::atomic::Ordering::AcqRel);

                    let results = &results;
                    let active = &active;
                    let provider = Arc::clone(provider);
                    let feed = feed.clone();
                    let provider_config = provider_config.clone();
                    let idx = *idx;
                    let ctx_ref = &ctx;
                    let asset_ref: &ResolvedAsset = asset;

                    scope.spawn(move || {
                        let result = provider.fetch(asset_ref, &feed, &provider_config, ctx_ref);
                        // Lock is infallible unless a sibling thread panicked.
                        if let Ok(mut guard) = results.lock() {
                            guard.push((idx, result));
                        }
                        active.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                    });
                }
            });

            // Sort by original index and collect.
            let indexed_results = results
                .into_inner()
                .map_err(|e| Error::Provider(format!("mutex poisoned: {e}")))?;
            let mut sorted_by_index = indexed_results;
            sorted_by_index.sort_by_key(|(i, _)| *i);

            for (idx, result) in sorted_by_index {
                match result {
                    Ok(candidate) => candidates.push(candidate),
                    Err(err) => {
                        if idx < sorted_feeds.len() {
                            provider_failures.push((sorted_feeds[idx].provider.clone(), err));
                        }
                    }
                }
            }
        }

        // Record provider failures as audit events.
        for (provider_id, err) in provider_failures {
            self.record_provider_failure(asset, &provider_id, &err)?;
        }

        let min = self.config.safety.consensus.min_successful_feeds;
        if candidates.len() < min {
            return Err(Error::Provider(format!(
                "insufficient successful feeds: {}/{}, asset: {}",
                candidates.len(),
                min,
                asset.id.as_str()
            )));
        }

        Ok(candidates)
    }

    /// Fetch a single candidate from one feed.
    fn fetch_single_candidate(
        &self,
        asset: &ResolvedAsset,
        feed: &ResolvedFeed,
    ) -> Result<CandidateRate> {
        let observed_at = OffsetDateTime::now_utc();
        let ctx = ProviderContext {
            quote: self.config.oracles.quote.clone(),
            observed_at,
            user_agent: self.config.http.user_agent.clone(),
            request_timeout_secs: self.config.http.request_timeout_secs,
            max_retries: self.config.http.max_retries,
            retry_backoff_ms: self.config.http.retry_backoff_ms,
        };
        let provider_config = self.config.provider(&feed.provider).ok_or_else(|| {
            Error::Config(format!("unknown provider: {}", feed.provider.as_str()))
        })?;
        let provider = self
            .providers
            .iter()
            .find(|p| p.id() == &feed.provider)
            .ok_or_else(|| {
                Error::Provider(format!(
                    "provider not registered: {}",
                    feed.provider.as_str()
                ))
            })?;
        provider.fetch(asset, feed, provider_config, &ctx)
    }

    /// Select the median candidate by rate value.
    ///
    /// Caller guarantees that `candidates` is non-empty.
    fn select_median_candidate(candidates: &[CandidateRate]) -> &CandidateRate {
        // Safety: caller ensures candidates is non-empty.
        let mut indices: Vec<usize> = (0..candidates.len()).collect();
        indices.sort_by(|&a, &b| {
            candidates[a]
                .rate
                .decimal()
                .cmp(&candidates[b].rate.decimal())
        });
        &candidates[indices[indices.len() / 2]]
    }

    fn apply_decision(&mut self, decision: Decision) -> Result<()> {
        match decision {
            Decision::Accept(record) => {
                self.store.write_accepted_rate(&record)?;
            }
            Decision::Alert { record, event } => {
                self.store.write_accepted_rate(&record)?;
                self.write_and_route_event(&event)?;
            }
            Decision::Quarantine(event)
            | Decision::Reject(event)
            | Decision::DisableAsset(event) => {
                self.write_and_route_event(&event)?;
            }
        }
        Ok(())
    }

    /// Write an event to the store and route it to configured sinks.
    ///
    /// Handles three delivery modes:
    /// - **Outbox mode**: writes event + pending outbox deliveries.
    /// - **Simple mode**: writes event + delivers directly to sinks.
    ///
    /// Respects `alert_cooldown_secs`: if a recent event of the same type
    /// exists for the same asset/provider, delivery to sinks is suppressed
    /// but the event is still recorded to the store.
    ///
    /// When `record_anomalies` is false, the event is still routed to sinks
    /// but skipped for database recording.
    fn write_and_route_event(&mut self, event: &OracleEvent) -> Result<()> {
        if !self.config.events.enabled {
            return Ok(());
        }

        // Check cooldown BEFORE writing the event to avoid self-suppression.
        let cooldown_secs = self.config.safety.alert_cooldown_secs;
        let suppress_delivery = cooldown_secs > 0
            && self.store.has_recent_event(
                &event.asset_id,
                &event.provider,
                &event.event_type,
                &event.reason,
                cooldown_secs,
            )?;

        // Always record the event for audit if recording is enabled.
        // Anomaly events (rate anomaly, quarantine, reject) are gated on
        // record_anomalies, while ProviderFailed and RefreshFailed are
        // always recorded (they form the durable audit trail).
        let is_anomaly = matches!(
            event.event_type,
            EventType::RateAnomaly | EventType::RateQuarantined | EventType::RateRejected
        );
        let should_record =
            self.config.events.record && (self.config.safety.record_anomalies || !is_anomaly);
        let event_id = if should_record {
            self.store.write_event(event)?
        } else {
            None
        };

        // If in cooldown, skip delivery but keep the audit record.
        if suppress_delivery {
            return Ok(());
        }

        // Route to sinks based on delivery mode.
        match self.config.events.mode {
            crate::config::EventMode::Simple => {
                let sink_names = self.config.events.sinks_for(&event.event_type);
                for sink_name in sink_names {
                    let payload = self.config.events.render_payload(sink_name, event)?;
                    let sink_config = self
                        .config
                        .events
                        .sinks
                        .get(sink_name)
                        .ok_or_else(|| Error::Config(format!("unknown sink: {sink_name}")))?;
                    let sink = crate::events::sinks::build_sink(sink_config)?;
                    if let Err(err) = sink.deliver(&payload) {
                        if self.config.events.sink_fail_fast {
                            return Err(err);
                        }
                        crate::error!("sink delivery failed for `{sink_name}`: {err}");
                    }
                }
            }
            crate::config::EventMode::Outbox => {
                for sink_name in self.config.events.sinks_for(&event.event_type) {
                    // Skip outbox rows for table sinks: the event is already
                    // persisted by write_event(), so a delivery row would be
                    // a no-op.
                    let sink_config = self.config.events.sinks.get(sink_name);
                    if let Some(crate::config::ResolvedEventSink::Table) = sink_config {
                        continue;
                    }
                    let payload = self.config.events.render_payload(sink_name, event)?;
                    self.store
                        .write_outbox(event_id, event, sink_name, &payload)?;
                }
            }
        }

        Ok(())
    }

    /// Write and route an event within a transaction.
    ///
    /// Used for events emitted outside the normal safety-decision flow
    /// (provider failures, refresh failures) to ensure atomicity in
    /// outbox mode.
    fn record_event_transactionally(&mut self, event: &OracleEvent) -> Result<()> {
        self.store.begin_decision()?;
        match self.write_and_route_event(event) {
            Ok(()) => {
                self.store.commit_decision()?;
                Ok(())
            }
            Err(err) => {
                let _ = self.store.rollback_decision();
                Err(err)
            }
        }
    }

    /// Record a provider failure as an audit event and route to sinks.
    fn record_provider_failure(
        &mut self,
        asset: &ResolvedAsset,
        provider: &ProviderId,
        err: &Error,
    ) -> Result<()> {
        if !self.config.events.enabled {
            return Ok(());
        }

        let observed_at = OffsetDateTime::now_utc();
        let failure_event = OracleEvent {
            event_type: EventType::ProviderFailed,
            asset_id: asset.id.clone(),
            chain_id: Some(asset.chain_id.clone()),
            symbol: asset.symbol.clone(),
            quote: self.config.oracles.quote.clone(),
            provider: provider.clone(),
            previous_rate: None,
            candidate_rate: None,
            change_pct: None,
            action: EventAction::Reject,
            reason: EventReason::ProviderError,
            source_updated_at: None,
            observed_at,
        };

        self.record_event_transactionally(&failure_event)?;

        crate::warn!(
            "provider `{provider}` failed for asset `{}`: {err}",
            asset.id.as_str()
        );

        Ok(())
    }

    /// Record a refresh failure as an audit event when all providers fail.
    fn record_refresh_failed(&mut self, asset: &ResolvedAsset, err: &Error) -> Result<()> {
        if !self.config.events.enabled {
            return Ok(());
        }

        let observed_at = OffsetDateTime::now_utc();
        let provider = ProviderId::new("oracles")?;
        let failure_event = OracleEvent {
            event_type: EventType::RefreshFailed,
            asset_id: asset.id.clone(),
            chain_id: Some(asset.chain_id.clone()),
            symbol: asset.symbol.clone(),
            quote: self.config.oracles.quote.clone(),
            provider,
            previous_rate: None,
            candidate_rate: None,
            change_pct: None,
            action: EventAction::Reject,
            reason: EventReason::ProviderError,
            source_updated_at: None,
            observed_at,
        };

        self.record_event_transactionally(&failure_event)?;

        crate::error!("refresh failed for asset `{}`: {err}", asset.id.as_str());

        Ok(())
    }
}

/// Summary of a single refresh cycle.
#[derive(Default)]
pub struct RefreshSummary {
    /// Total number of assets that were attempted.
    pub attempted: usize,
    /// Number of assets that were successfully refreshed.
    pub succeeded: usize,
    /// Number of assets that failed to refresh.
    pub failed: usize,
}

impl RefreshSummary {
    /// Record a single asset attempt outcome.
    pub fn record(&mut self, _asset_id: &crate::domain::AssetId, ok: bool) {
        self.attempted += 1;
        if ok {
            self.succeeded += 1;
        } else {
            self.failed += 1;
        }
    }
}

/// Summary of a single outbox dispatch pass.
#[derive(Clone, Debug, Default)]
pub struct DispatchOutboxSummary {
    /// Total deliveries attempted.
    pub attempted: usize,
    /// Deliveries that succeeded.
    pub delivered: usize,
    /// Deliveries that failed but will be retried.
    pub failed: usize,
    /// Deliveries that exceeded max retries and are now dead.
    pub dead: usize,
}
