-- Migration 001: oracle_rates table
CREATE TABLE IF NOT EXISTS oracle_rates (
    id BIGSERIAL PRIMARY KEY,
    asset_id TEXT NOT NULL,
    chain_id TEXT NOT NULL,
    caip2 TEXT NOT NULL,
    symbol TEXT NOT NULL,
    quote TEXT NOT NULL,
    provider TEXT NOT NULL,
    rate TEXT NOT NULL,
    source_updated_at TIMESTAMPTZ,
    observed_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS oracle_rates_asset_quote_idx ON oracle_rates (asset_id, quote);
CREATE INDEX IF NOT EXISTS oracle_rates_expires_at_idx ON oracle_rates (expires_at);
-- Unique constraint for upsert mode only.
-- For append mode, do NOT create this unique index, as multiple rows per
-- (asset_id, quote, provider) are expected.
-- If using upsert mode, uncomment the following line:
-- CREATE UNIQUE INDEX IF NOT EXISTS oracle_rates_asset_quote_provider_uniq ON oracle_rates (asset_id, quote, provider);
