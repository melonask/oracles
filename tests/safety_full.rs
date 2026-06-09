#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::type_complexity
)]
#![allow(missing_docs)]

use oracles::config::{
    BootstrapAction, CompareAgainst, ResolvedAsset, ResolvedConsensusConfig, ResolvedSafetyConfig,
};
use oracles::domain::{
    AssetId, CandidateRate, ChainId, EventAction, EventReason, ProviderId, Quote, RateAmount,
    RateRecord,
};
use oracles::safety::SafetyEngine;
use rust_decimal::Decimal;
use time::{Duration, OffsetDateTime};

fn make_asset(
    id: &str,
    overrides: Option<(
        Option<Decimal>,
        Option<RateAmount>,
        Option<RateAmount>,
        Option<EventAction>,
    )>,
) -> ResolvedAsset {
    let (max_change, min_rate, max_rate, action) = overrides.unwrap_or((None, None, None, None));
    ResolvedAsset {
        id: AssetId::new(id).unwrap(),
        enabled: true,
        chain_id: ChainId::new("eth").unwrap(),
        caip2: "eip155:1".to_owned(),
        symbol: "ETH".to_owned(),
        name: None,
        kind: "native".to_owned(),
        contract: None,
        decimals: 18,
        x402: None,
        feeds: vec![],
        safety_enabled: true,
        safety_max_change_pct: max_change,
        safety_min_rate: min_rate,
        safety_max_rate: max_rate,
        safety_action: action,
    }
}

fn make_candidate(
    asset: &ResolvedAsset,
    rate: &str,
    source_age_secs: Option<i64>,
) -> CandidateRate {
    let now = OffsetDateTime::now_utc();
    let source = source_age_secs.map(|s| now - Duration::seconds(s));
    CandidateRate {
        asset_id: asset.id.clone(),
        chain_id: asset.chain_id.clone(),
        caip2: asset.caip2.clone(),
        symbol: asset.symbol.clone(),
        quote: Quote::new("USD").unwrap(),
        provider: ProviderId::new("test_provider").unwrap(),
        rate: RateAmount::parse(rate).unwrap(),
        source_updated_at: source,
        observed_at: now,
    }
}

fn default_safety_config() -> ResolvedSafetyConfig {
    ResolvedSafetyConfig {
        enabled: true,
        compare_against: CompareAgainst::LastAccepted,
        default_action: EventAction::Quarantine,
        max_change_pct: Decimal::new(50, 0), // 50%
        min_rate: None,
        max_rate: None,
        max_source_age: Some(Duration::seconds(900)),
        stale_after: Duration::seconds(300),
        alert_cooldown_secs: 3600,
        record_anomalies: true,
        bootstrap_action: BootstrapAction::Accept,
        consensus: ResolvedConsensusConfig {
            min_successful_feeds: 1,
            max_provider_spread_pct: Decimal::new(5, 0),
            action: EventAction::Quarantine,
        },
    }
}

// ---------------------------------------------------------------------------
// Safety: Accept path
// ---------------------------------------------------------------------------

#[test]
fn safety_accepts_normal_rate() {
    let engine = SafetyEngine::new(default_safety_config());
    let asset = make_asset("eth", None);
    let candidate = make_candidate(&asset, "3500.25", None);
    let result = engine
        .evaluate(&asset, None, candidate, OffsetDateTime::now_utc())
        .unwrap();
    assert!(matches!(result, oracles::domain::Decision::Accept(_)));
}

#[test]
fn safety_accepts_rate_with_valid_previous_change() {
    let mut config = default_safety_config();
    config.max_change_pct = Decimal::new(10, 0); // 10%
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let candidate = make_candidate(&asset, "110", None);
    let previous = RateRecord {
        asset_id: asset.id.clone(),
        chain_id: asset.chain_id.clone(),
        caip2: asset.caip2.clone(),
        symbol: asset.symbol.clone(),
        quote: Quote::new("USD").unwrap(),
        provider: ProviderId::new("test_provider").unwrap(),
        rate: RateAmount::parse("100").unwrap(),
        source_updated_at: Some(OffsetDateTime::now_utc() - Duration::seconds(60)),
        observed_at: OffsetDateTime::now_utc() - Duration::seconds(60),
        expires_at: OffsetDateTime::now_utc() + Duration::seconds(240),
    };
    let result = engine
        .evaluate(
            &asset,
            Some(&previous),
            candidate,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    assert!(matches!(result, oracles::domain::Decision::Accept(_)));
}

// ---------------------------------------------------------------------------
// Safety: Source age check
// ---------------------------------------------------------------------------

#[test]
fn safety_rejects_stale_source() {
    let config = default_safety_config();
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let candidate = make_candidate(&asset, "3500.25", Some(1000)); // 1000s old > 900s max
    let result = engine
        .evaluate(&asset, None, candidate, OffsetDateTime::now_utc())
        .unwrap();
    match result {
        oracles::domain::Decision::Quarantine(event) => {
            assert_eq!(event.reason, EventReason::SourceTimestampTooOld);
        }
        other => panic!("expected Quarantine, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Safety: Rate bounds
// ---------------------------------------------------------------------------

#[test]
fn safety_rejects_rate_below_min() {
    let mut config = default_safety_config();
    config.min_rate = Some(RateAmount::parse("1").unwrap());
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let candidate = make_candidate(&asset, "0.5", None);
    let result = engine
        .evaluate(&asset, None, candidate, OffsetDateTime::now_utc())
        .unwrap();
    match result {
        oracles::domain::Decision::Quarantine(event) => {
            assert_eq!(event.reason, EventReason::RateBelowMin);
        }
        other => panic!("expected Quarantine for below min, got {other:?}"),
    }
}

#[test]
fn safety_rejects_rate_above_max() {
    let mut config = default_safety_config();
    config.max_rate = Some(RateAmount::parse("100000").unwrap());
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let candidate = make_candidate(&asset, "200000", None);
    let result = engine
        .evaluate(&asset, None, candidate, OffsetDateTime::now_utc())
        .unwrap();
    match result {
        oracles::domain::Decision::Quarantine(event) => {
            assert_eq!(event.reason, EventReason::RateAboveMax);
        }
        other => panic!("expected Quarantine for above max, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Safety: Max change exceeded
// ---------------------------------------------------------------------------

#[test]
fn safety_quarantines_large_change() {
    let mut config = default_safety_config();
    config.max_change_pct = Decimal::new(5, 0); // 5%
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let candidate = make_candidate(&asset, "120", None);
    let previous = RateRecord {
        asset_id: asset.id.clone(),
        chain_id: asset.chain_id.clone(),
        caip2: asset.caip2.clone(),
        symbol: asset.symbol.clone(),
        quote: Quote::new("USD").unwrap(),
        provider: ProviderId::new("test_provider").unwrap(),
        rate: RateAmount::parse("100").unwrap(),
        source_updated_at: Some(OffsetDateTime::now_utc() - Duration::seconds(60)),
        observed_at: OffsetDateTime::now_utc() - Duration::seconds(60),
        expires_at: OffsetDateTime::now_utc() + Duration::seconds(240),
    };
    let result = engine
        .evaluate(
            &asset,
            Some(&previous),
            candidate,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    match result {
        oracles::domain::Decision::Quarantine(event) => {
            assert_eq!(event.reason, EventReason::MaxChangeExceeded);
        }
        other => panic!("expected Quarantine for max change, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Safety: Bootstrap actions
// ---------------------------------------------------------------------------

#[test]
fn safety_bootstrap_accept() {
    let mut config = default_safety_config();
    config.bootstrap_action = BootstrapAction::Accept;
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let candidate = make_candidate(&asset, "3500", None);
    let result = engine
        .evaluate(&asset, None, candidate, OffsetDateTime::now_utc())
        .unwrap();
    assert!(matches!(result, oracles::domain::Decision::Accept(_)));
}

#[test]
fn safety_bootstrap_quarantine() {
    let mut config = default_safety_config();
    config.bootstrap_action = BootstrapAction::Quarantine;
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let candidate = make_candidate(&asset, "3500", None);
    let result = engine
        .evaluate(&asset, None, candidate, OffsetDateTime::now_utc())
        .unwrap();
    match result {
        oracles::domain::Decision::Quarantine(event) => {
            assert_eq!(event.reason, EventReason::MissingPreviousRate);
        }
        other => panic!("expected Quarantine for bootstrap, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Safety: Consensus spread
// ---------------------------------------------------------------------------

#[test]
fn consensus_passes_when_spread_is_small() {
    let config = default_safety_config();
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let c1 = make_candidate(&asset, "3500", None);
    let c2 = make_candidate(&asset, "3510", None); // ~0.29% spread
    let result = engine.check_consensus(&asset, &[c1, c2], None).unwrap();
    assert!(result.is_none(), "expected no consensus action");
}

#[test]
fn consensus_triggers_on_large_spread() {
    let mut config = default_safety_config();
    config.consensus.max_provider_spread_pct = Decimal::new(1, 0); // 1%
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let c1 = make_candidate(&asset, "3500", None);
    let c2 = make_candidate(&asset, "3600", None); // ~2.86% spread
    let result = engine.check_consensus(&asset, &[c1, c2], None).unwrap();
    assert!(result.is_some(), "expected consensus violation");
    if let Some(decision) = result {
        match decision {
            oracles::domain::Decision::Quarantine(event) => {
                assert_eq!(event.reason, EventReason::ProviderSpreadExceeded);
            }
            other => panic!("expected Quarantine for spread, got {other:?}"),
        }
    }
}

#[test]
fn consensus_skip_with_single_candidate() {
    let config = default_safety_config();
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let c1 = make_candidate(&asset, "3500", None);
    let result = engine.check_consensus(&asset, &[c1], None).unwrap();
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// Safety: Alert action produces Alert decision
// ---------------------------------------------------------------------------

#[test]
fn safety_alert_action_produces_alert_decision() {
    let mut config = default_safety_config();
    config.default_action = EventAction::Alert;
    let engine = SafetyEngine::new(config);
    let asset = make_asset("eth", None);
    let candidate = make_candidate(&asset, "3500", Some(1000));
    let result = engine
        .evaluate(&asset, None, candidate, OffsetDateTime::now_utc())
        .unwrap();
    match result {
        oracles::domain::Decision::Alert { record, event } => {
            assert!(event.reason == EventReason::SourceTimestampTooOld);
            assert!(record.rate.decimal() > Decimal::ZERO);
        }
        other => panic!("expected Alert decision, got {other:?}"),
    }
}
