-- Migration 001: oracle_rates table
CREATE TABLE IF NOT EXISTS oracle_rates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id TEXT NOT NULL,
    chain_id TEXT NOT NULL,
    caip2 TEXT NOT NULL,
    symbol TEXT NOT NULL,
    quote TEXT NOT NULL,
    provider TEXT NOT NULL,
    rate TEXT NOT NULL,
    source_updated_at TEXT,
    observed_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS oracle_rates_asset_quote_idx ON oracle_rates (asset_id, quote);
CREATE INDEX IF NOT EXISTS oracle_rates_expires_at_idx ON oracle_rates (expires_at);
