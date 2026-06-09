-- Migration 003: oracle_outbox table
CREATE TABLE IF NOT EXISTS oracle_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id INTEGER,
    sink TEXT NOT NULL,
    payload TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,
    delivered_at TEXT,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS oracle_outbox_status_next_idx ON oracle_outbox (status, next_attempt_at);
