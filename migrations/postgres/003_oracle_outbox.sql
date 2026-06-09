-- Migration 003: oracle_outbox table
CREATE TABLE IF NOT EXISTS oracle_outbox (
    id BIGSERIAL PRIMARY KEY,
    event_id BIGINT,
    sink TEXT NOT NULL,
    payload TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL,
    delivered_at TIMESTAMPTZ,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS oracle_outbox_status_next_idx ON oracle_outbox (status, next_attempt_at);
