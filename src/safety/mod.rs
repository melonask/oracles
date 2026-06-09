use crate::config::{BootstrapAction, ResolvedAsset, ResolvedSafetyConfig};
use crate::domain::{
    CandidateRate, Decision, EventAction, EventReason, EventType, OracleEvent, RateRecord,
};
use crate::error::Result;
use rust_decimal::Decimal;
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

/// The safety engine that evaluates candidate rates against configured rules.
///
/// Applies checks in order: source age, rate bounds (min/max), and percent
/// change from the previous rate. Each check can produce a [`Decision`]
/// ranging from `Accept` to `DisableAsset`.
pub struct SafetyEngine {
    config: ResolvedSafetyConfig,
}

impl SafetyEngine {
    /// Create a new [`SafetyEngine`] from a resolved safety configuration.
    pub fn new(config: ResolvedSafetyConfig) -> Self {
        Self { config }
    }

    /// Evaluate a candidate rate against the safety rules.
    ///
    /// Takes the asset definition (for per-asset overrides), an optional
    /// previous rate record, the candidate rate, and the current time.
    /// Returns a [`Decision`] indicating whether the rate should be
    /// accepted, alerted, quarantined, rejected, or the asset disabled.
    pub fn evaluate(
        &self,
        asset: &ResolvedAsset,
        previous: Option<&RateRecord>,
        candidate: CandidateRate,
        now: OffsetDateTime,
    ) -> Result<Decision> {
        // 1. Check source age
        if let Some(max_age) = self.config.max_source_age
            && let Some(source_time) = candidate.source_updated_at
        {
            let age = now - source_time;
            if age > max_age {
                return Ok(self.event_decision(
                    asset,
                    previous,
                    &candidate,
                    EventReason::SourceTimestampTooOld,
                    None,
                ));
            }
        }

        // 2. Check rate bounds (asset-specific overrides global)
        if let Some(min_rate) = asset
            .safety_min_rate
            .as_ref()
            .or(self.config.min_rate.as_ref())
            && candidate.rate.decimal() < min_rate.decimal()
        {
            return Ok(self.event_decision(
                asset,
                previous,
                &candidate,
                EventReason::RateBelowMin,
                None,
            ));
        }

        if let Some(max_rate) = asset
            .safety_max_rate
            .as_ref()
            .or(self.config.max_rate.as_ref())
            && candidate.rate.decimal() > max_rate.decimal()
        {
            return Ok(self.event_decision(
                asset,
                previous,
                &candidate,
                EventReason::RateAboveMax,
                None,
            ));
        }

        // 3. Check percent change against previous rate
        if let Some(previous) = previous {
            let change_pct = candidate.rate.percent_change_from(&previous.rate)?;
            let max_change = asset
                .safety_max_change_pct
                .unwrap_or(self.config.max_change_pct);

            if change_pct > max_change {
                return Ok(self.event_decision(
                    asset,
                    Some(previous),
                    &candidate,
                    EventReason::MaxChangeExceeded,
                    Some(change_pct),
                ));
            }
        } else {
            match self.config.bootstrap_action {
                BootstrapAction::Accept => { /* fall through to accept below */ }
                BootstrapAction::Quarantine => {
                    return Ok(self.event_decision(
                        asset,
                        None,
                        &candidate,
                        EventReason::MissingPreviousRate,
                        None,
                    ));
                }
                BootstrapAction::RequireMultipleProviders => {
                    // Engine-level enforcement (min_successful_feeds in
                    // fetch_all_candidates, plus explicit checks in
                    // refresh_asset_priority / refresh_asset_all /
                    // refresh_asset_median) ensures multiple providers
                    // are available before this point. The safety
                    // engine accepts the rate value itself.
                    /* fall through to accept below */
                }
            }
        }

        // 4. Accept: build RateRecord with computed expires_at
        let expires_at = candidate.observed_at + self.config.stale_after;

        Ok(Decision::Accept(RateRecord {
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
        }))
    }

    /// Check that multiple provider rates don't diverge too far.
    ///
    /// Returns `Ok(Some(Decision))` if the spread exceeds the configured
    /// maximum, or `Ok(None)` if consensus passes.
    pub fn check_consensus(
        &self,
        _asset: &ResolvedAsset,
        candidates: &[CandidateRate],
        previous: Option<&RateRecord>,
    ) -> Result<Option<Decision>> {
        // If fewer than 2 candidates, there's no spread to check
        if candidates.len() < 2 {
            return Ok(None);
        }

        // Find min and max rates manually to avoid unwrap/expect
        let first = &candidates[0];
        let mut min_rate = first.rate.decimal();
        let mut max_rate = first.rate.decimal();
        for c in &candidates[1..] {
            let r = c.rate.decimal();
            if r < min_rate {
                min_rate = r;
            }
            if r > max_rate {
                max_rate = r;
            }
        }

        // Guard against zero/negative rates (shouldn't happen with RateAmount but be safe)
        if min_rate <= Decimal::ZERO {
            return Ok(None);
        }

        let spread_pct = (max_rate - min_rate) / min_rate * Decimal::new(100, 0);

        if spread_pct > self.config.consensus.max_provider_spread_pct {
            let event = OracleEvent {
                event_type: event_type_for_action(&self.config.consensus.action),
                asset_id: first.asset_id.clone(),
                chain_id: Some(first.chain_id.clone()),
                symbol: first.symbol.clone(),
                quote: first.quote.clone(),
                provider: first.provider.clone(),
                previous_rate: previous.map(|r| r.rate.clone()),
                candidate_rate: Some(first.rate.clone()),
                change_pct: Some(spread_pct),
                action: self.config.consensus.action.clone(),
                reason: EventReason::ProviderSpreadExceeded,
                source_updated_at: first.source_updated_at,
                observed_at: first.observed_at,
            };

            let decision = match self.config.consensus.action {
                EventAction::Alert => {
                    let expires_at = first.observed_at + self.config.stale_after;
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

            Ok(Some(decision))
        } else {
            Ok(None)
        }
    }

    fn event_decision(
        &self,
        asset: &ResolvedAsset,
        previous: Option<&RateRecord>,
        candidate: &CandidateRate,
        reason: EventReason,
        change_pct: Option<Decimal>,
    ) -> Decision {
        // Asset-specific action overrides global default
        let action = asset
            .safety_action
            .clone()
            .unwrap_or_else(|| self.config.default_action.clone());

        let event = OracleEvent {
            event_type: event_type_for_action(&action),
            asset_id: candidate.asset_id.clone(),
            chain_id: Some(candidate.chain_id.clone()),
            symbol: candidate.symbol.clone(),
            quote: candidate.quote.clone(),
            provider: candidate.provider.clone(),
            previous_rate: previous.map(|r| r.rate.clone()),
            candidate_rate: Some(candidate.rate.clone()),
            change_pct,
            action: action.clone(),
            reason,
            source_updated_at: candidate.source_updated_at,
            observed_at: candidate.observed_at,
        };

        match action {
            EventAction::Alert => {
                let expires_at = candidate.observed_at + self.config.stale_after;
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
    }
}
