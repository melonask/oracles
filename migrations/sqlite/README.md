SQLite migration files.

These SQL files define the expected schema for the `oracles` service. They are
provided as a reference for external migration tools or manual execution.

**Important:** The in-code `SqliteRateStore::migrate()` method creates the same
tables and indexes using `CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT
EXISTS`. If `migrate = true` in the config, the service will run these migrations
automatically on startup.

If you set `migrate = false`, you must apply these migration files manually
before starting the service. The in-code migrations and these files must be
kept in sync.
