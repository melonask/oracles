-- Migration 002: oracle_events table
CREATE TABLE IF NOT EXISTS oracle_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    chain_id TEXT,
    symbol TEXT NOT NULL,
    quote TEXT NOT NULL,
    provider TEXT NOT NULL,
    previous_rate TEXT,
    candidate_rate TEXT,
    change_pct TEXT,
    action TEXT NOT NULL,
    reason TEXT NOT NULL,
    source_updated_at TEXT,
    observed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS oracle_events_asset_quote_idx ON oracle_events (asset_id, quote);
CREATE INDEX IF NOT EXISTS oracle_events_observed_at_idx ON oracle_events (observed_at);
