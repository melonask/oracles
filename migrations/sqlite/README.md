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

## write_mode and the unique index

The `(asset_id, quote, provider)` unique index on the `oracle_rates` table is
only needed for `write_mode = "upsert"`. In append mode, multiple rows per
`(asset_id, quote, provider)` are expected and the unique index would prevent
inserts.

- **Upsert mode**: Uncomment the unique index in `001_oracle_rates.sql` before
  applying the migration. The in-code `migrate()` method creates it
  automatically.
- **Append mode**: Leave the unique index commented out. The in-code
  `migrate()` method skips it automatically.

If an existing SQLite database previously ran in upsert mode, the unique index
may already exist. Before switching that same database to append mode, drop
`oracle_rates_asset_quote_provider_uniq` manually; otherwise SQLite will
continue rejecting multiple rows for the same `(asset_id, quote, provider)`.
