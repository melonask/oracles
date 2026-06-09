use crate::config::{
    ResolvedConfig, ResolvedEventColumns, ResolvedOutboxColumns, ResolvedRateColumns, StoreDriver,
    WriteMode,
};
use crate::domain::{
    AssetId, ChainId, EventAction, EventReason, EventType, OracleEvent, ProviderId, Quote,
    RateAmount, RateRecord,
};
use crate::error::{Error, Result};
use crate::store::{EventRowId, OutboxDelivery, OutboxStore, RateStore};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// A SQLite-backed implementation of [`RateStore`] and [`OutboxStore`].
///
/// Supports both file-based and in-memory (`:memory:`) databases. Uses
/// upsert semantics for rate writes and stores timestamps in RFC 3339
/// format.
pub struct SqliteRateStore {
    conn: Connection,
    rates_table: String,
    events_table: String,
    outbox_table: String,
    in_tx: bool,
    write_mode: WriteMode,
    #[allow(dead_code)]
    stale_after_secs: u64,
    /// Configured column names for the rates table.
    rate_columns: ResolvedRateColumns,
    /// Configured column names for the events table.
    event_columns: ResolvedEventColumns,
    /// Configured column names for the outbox table.
    outbox_columns: ResolvedOutboxColumns,
}

impl SqliteRateStore {
    /// Open a SQLite store from a resolved configuration.
    ///
    /// Reads the store URL, table names, and migration flag from the
    /// config. Creates parent directories for file-based paths and runs
    /// schema migrations if `migrate` is enabled.
    pub fn open(config: &ResolvedConfig) -> Result<Self> {
        let store_config = config
            .stores
            .get(&config.oracles.store)
            .ok_or_else(|| Error::Store(format!("store not found: {}", config.oracles.store)))?;

        if store_config.driver != StoreDriver::Sqlite {
            return Err(Error::Store("store driver must be Sqlite".to_owned()));
        }

        if store_config.max_connections != 1 {
            return Err(Error::Store(format!(
                "SQLite store only supports max_connections = 1, got: {}. \
                 For connection pooling, use PostgreSQL.",
                store_config.max_connections
            )));
        }

        let write_mode = config.oracles.table.write_mode.clone();
        let stale_after_secs = config.oracles.stale_after_secs;

        let path = sqlite_path_from_url(&store_config.url)?;

        let rates_table = validate_identifier(&config.oracles.table.name, "oracles.table.name")?;
        let events_table = validate_identifier(&config.events.table, "events.table")?;
        let outbox_table = validate_identifier(&config.outbox.table, "outbox.table")?;

        // Extract column name mappings from the resolved config.
        let rate_columns = config.oracles.table.columns.clone();
        let event_columns = config.events.columns.clone();
        let outbox_columns = config.outbox.columns.clone();

        // Create parent directories if using a file-based path.
        if path != ":memory:"
            && let Some(parent) = Path::new(&path).parent()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                Error::Store(format!(
                    "failed to create parent directories for sqlite path: {e}"
                ))
            })?;
        }

        let conn = Connection::open(&path)
            .map_err(|e| Error::Store(format!("failed to open sqlite database: {e}")))?;

        conn.busy_timeout(std::time::Duration::from_secs(
            store_config.connect_timeout_secs,
        ))
        .map_err(|e| Error::Store(format!("failed to set busy timeout: {e}")))?;

        let store = Self {
            conn,
            rates_table,
            events_table,
            outbox_table,
            in_tx: false,
            write_mode,
            stale_after_secs,
            rate_columns,
            event_columns,
            outbox_columns,
        };

        if store_config.migrate {
            store.migrate()?;
        }

        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(|e| Error::Store(format!("failed to set pragmas: {e}")))?;

        let rc = &self.rate_columns;
        let ec = &self.event_columns;
        let oc = &self.outbox_columns;

        // -- rates table --
        let create_rates = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                {} INTEGER PRIMARY KEY AUTOINCREMENT,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL
            )",
            self.rates_table,
            rc.id,
            rc.asset_id,
            rc.chain_id,
            rc.caip2,
            rc.symbol,
            rc.quote,
            rc.provider,
            rc.rate,
            rc.source_updated_at,
            rc.observed_at,
            rc.expires_at,
        );
        self.conn
            .execute(&create_rates, [])
            .map_err(|e| Error::Store(format!("failed to create rates table: {e}")))?;

        let rates_asset_quote_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_asset_quote_idx ON {t} ({asset_id}, {quote})",
            t = self.rates_table,
            asset_id = rc.asset_id,
            quote = rc.quote,
        );
        self.conn
            .execute(&rates_asset_quote_idx, [])
            .map_err(|e| Error::Store(format!("failed to create rates asset_quote index: {e}")))?;

        let rates_expires_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_expires_at_idx ON {t} ({expires_at})",
            t = self.rates_table,
            expires_at = rc.expires_at,
        );
        self.conn
            .execute(&rates_expires_idx, [])
            .map_err(|e| Error::Store(format!("failed to create rates expires_at index: {e}")))?;

        // -- events table --
        let create_events = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                {} INTEGER PRIMARY KEY AUTOINCREMENT,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT,
                {} TEXT,
                {} TEXT,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT,
                {} TEXT NOT NULL
            )",
            self.events_table,
            ec.id,
            ec.event_type,
            ec.asset_id,
            ec.chain_id,
            ec.symbol,
            ec.quote,
            ec.provider,
            ec.previous_rate,
            ec.candidate_rate,
            ec.change_pct,
            ec.action,
            ec.reason,
            ec.source_updated_at,
            ec.observed_at,
        );
        self.conn
            .execute(&create_events, [])
            .map_err(|e| Error::Store(format!("failed to create events table: {e}")))?;

        let events_asset_quote_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_asset_quote_idx ON {t} ({asset_id}, {quote})",
            t = self.events_table,
            asset_id = ec.asset_id,
            quote = ec.quote,
        );
        self.conn
            .execute(&events_asset_quote_idx, [])
            .map_err(|e| Error::Store(format!("failed to create events asset_quote index: {e}")))?;

        let events_observed_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_observed_at_idx ON {t} ({observed_at})",
            t = self.events_table,
            observed_at = ec.observed_at,
        );
        self.conn
            .execute(&events_observed_idx, [])
            .map_err(|e| Error::Store(format!("failed to create events observed_at index: {e}")))?;

        // -- outbox table --
        let create_outbox = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                {} INTEGER PRIMARY KEY AUTOINCREMENT,
                {} INTEGER,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL DEFAULT 'pending',
                {} INTEGER NOT NULL DEFAULT 0,
                {} TEXT NOT NULL,
                {} TEXT,
                {} TEXT
            )",
            self.outbox_table,
            oc.id,
            oc.event_id,
            oc.sink,
            oc.payload,
            oc.status,
            oc.attempts,
            oc.next_attempt_at,
            oc.delivered_at,
            oc.last_error,
        );
        self.conn
            .execute(&create_outbox, [])
            .map_err(|e| Error::Store(format!("failed to create outbox table: {e}")))?;

        let outbox_status_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_status_next_idx ON {t} ({status}, {next_attempt_at})",
            t = self.outbox_table,
            status = oc.status,
            next_attempt_at = oc.next_attempt_at,
        );
        self.conn
            .execute(&outbox_status_idx, [])
            .map_err(|e| Error::Store(format!("failed to create outbox status_next index: {e}")))?;

        Ok(())
    }
}

impl RateStore for SqliteRateStore {
    fn begin_decision(&mut self) -> Result<()> {
        if self.in_tx {
            return Err(Error::Store("transaction already in progress".to_owned()));
        }
        // Retry BEGIN IMMEDIATE a few times in case of concurrent write
        // contention. The busy_timeout handles short waits, but explicit
        // retries cover cases where the WAL writer needs to start.
        let mut last_err = None;
        for _ in 0..3 {
            match self.conn.execute("BEGIN IMMEDIATE", []) {
                Ok(_) => {
                    self.in_tx = true;
                    return Ok(());
                }
                Err(e) => {
                    last_err = Some(e);
                    // Brief pause before retry
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
        Err(Error::Store(format!(
            "failed to begin transaction after 3 retries: {}",
            last_err.map_or_else(|| "unknown error".to_owned(), |e| e.to_string())
        )))
    }

    fn commit_decision(&mut self) -> Result<()> {
        self.conn
            .execute("COMMIT", [])
            .map_err(|e| Error::Store(format!("failed to commit transaction: {e}")))?;
        self.in_tx = false;
        Ok(())
    }

    fn rollback_decision(&mut self) -> Result<()> {
        self.conn
            .execute("ROLLBACK", [])
            .map_err(|e| Error::Store(format!("failed to rollback transaction: {e}")))?;
        self.in_tx = false;
        Ok(())
    }

    fn last_accepted_rate(
        &mut self,
        asset_id: &AssetId,
        quote: &Quote,
    ) -> Result<Option<RateRecord>> {
        let c = &self.rate_columns;
        let sql = format!(
            "SELECT {}, {}, {}, {}, {}, {}, {}, {}, {}, {} \
             FROM {} \
             WHERE {} = ?1 AND {} = ?2 \
             ORDER BY {} DESC, {} DESC \
             LIMIT 1",
            c.asset_id,
            c.chain_id,
            c.caip2,
            c.symbol,
            c.quote,
            c.provider,
            c.rate,
            c.source_updated_at,
            c.observed_at,
            c.expires_at,
            self.rates_table,
            c.asset_id,
            c.quote,
            c.observed_at,
            c.id,
        );

        let result: Option<Result<RateRecord>> = self
            .conn
            .query_row(
                &sql,
                params![asset_id.as_str(), quote.as_str()],
                row_to_rate_record,
            )
            .optional()
            .map_err(|e| Error::Store(format!("failed to query last accepted rate: {e}")))?;

        match result {
            Some(Ok(record)) => Ok(Some(record)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    fn last_accepted_rate_for_provider(
        &mut self,
        asset_id: &AssetId,
        quote: &Quote,
        provider: &ProviderId,
    ) -> Result<Option<RateRecord>> {
        let c = &self.rate_columns;
        let sql = format!(
            "SELECT {}, {}, {}, {}, {}, {}, {}, {}, {}, {} \
             FROM {} \
             WHERE {} = ?1 AND {} = ?2 AND {} = ?3 \
             ORDER BY {} DESC, {} DESC \
             LIMIT 1",
            c.asset_id,
            c.chain_id,
            c.caip2,
            c.symbol,
            c.quote,
            c.provider,
            c.rate,
            c.source_updated_at,
            c.observed_at,
            c.expires_at,
            self.rates_table,
            c.asset_id,
            c.quote,
            c.provider,
            c.observed_at,
            c.id,
        );

        let result: Option<Result<RateRecord>> = self
            .conn
            .query_row(
                &sql,
                params![asset_id.as_str(), quote.as_str(), provider.as_str()],
                row_to_rate_record,
            )
            .optional()
            .map_err(|e| {
                Error::Store(format!(
                    "failed to query last accepted rate for provider: {e}"
                ))
            })?;

        match result {
            Some(Ok(record)) => Ok(Some(record)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    fn write_accepted_rate(&mut self, record: &RateRecord) -> Result<()> {
        let source_updated_at = fmt_ts_opt(record.source_updated_at)?;
        let observed_at = fmt_ts(record.observed_at)?;
        let expires_at = fmt_ts(record.expires_at)?;
        let c = &self.rate_columns;

        match self.write_mode {
            WriteMode::Upsert => {
                // DELETE + INSERT within a transaction is atomic for SQLite
                // in WAL mode. The outer BEGIN IMMEDIATE locks the database
                // so no concurrent writer can see the transient missing row.
                let delete_sql = format!(
                    "DELETE FROM {} WHERE {} = ?1 AND {} = ?2 AND {} = ?3",
                    self.rates_table, c.asset_id, c.quote, c.provider,
                );
                self.conn
                    .execute(
                        &delete_sql,
                        params![
                            record.asset_id.as_str(),
                            record.quote.as_str(),
                            record.provider.as_str(),
                        ],
                    )
                    .map_err(|e| Error::Store(format!("failed to delete for upsert: {e}")))?;

                let insert_sql = format!(
                    "INSERT INTO {} \
                     ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    self.rates_table,
                    c.asset_id,
                    c.chain_id,
                    c.caip2,
                    c.symbol,
                    c.quote,
                    c.provider,
                    c.rate,
                    c.source_updated_at,
                    c.observed_at,
                    c.expires_at,
                );
                self.conn
                    .execute(
                        &insert_sql,
                        params![
                            record.asset_id.as_str(),
                            record.chain_id.as_str(),
                            &record.caip2,
                            &record.symbol,
                            record.quote.as_str(),
                            record.provider.as_str(),
                            record.rate.to_string(),
                            source_updated_at,
                            observed_at,
                            expires_at,
                        ],
                    )
                    .map_err(|e| Error::Store(format!("failed to write accepted rate: {e}")))?;
            }
            WriteMode::Append => {
                let insert_sql = format!(
                    "INSERT INTO {} \
                     ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    self.rates_table,
                    c.asset_id,
                    c.chain_id,
                    c.caip2,
                    c.symbol,
                    c.quote,
                    c.provider,
                    c.rate,
                    c.source_updated_at,
                    c.observed_at,
                    c.expires_at,
                );
                self.conn
                    .execute(
                        &insert_sql,
                        params![
                            record.asset_id.as_str(),
                            record.chain_id.as_str(),
                            &record.caip2,
                            &record.symbol,
                            record.quote.as_str(),
                            record.provider.as_str(),
                            record.rate.to_string(),
                            source_updated_at,
                            observed_at,
                            expires_at,
                        ],
                    )
                    .map_err(|e| Error::Store(format!("failed to write accepted rate: {e}")))?;
            }
        }

        Ok(())
    }

    fn write_event(&mut self, event: &OracleEvent) -> Result<EventRowId> {
        let ec = &self.event_columns;
        let sql = format!(
            "INSERT INTO {} \
             ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            self.events_table,
            ec.event_type,
            ec.asset_id,
            ec.chain_id,
            ec.symbol,
            ec.quote,
            ec.provider,
            ec.previous_rate,
            ec.candidate_rate,
            ec.change_pct,
            ec.action,
            ec.reason,
            ec.source_updated_at,
            ec.observed_at,
        );

        let chain_id = event.chain_id.as_ref().map(|c| c.as_str());
        let previous_rate = event.previous_rate.as_ref().map(|r| r.to_string());
        let candidate_rate = event.candidate_rate.as_ref().map(|r| r.to_string());
        let change_pct = event.change_pct.map(|d| d.to_string());
        let source_updated_at = fmt_ts_opt(event.source_updated_at)?;
        let observed_at = fmt_ts(event.observed_at)?;

        self.conn
            .execute(
                &sql,
                params![
                    event.event_type.as_str(),
                    event.asset_id.as_str(),
                    chain_id,
                    &event.symbol,
                    event.quote.as_str(),
                    event.provider.as_str(),
                    previous_rate,
                    candidate_rate,
                    change_pct,
                    event_action_as_str(&event.action),
                    event_reason_as_str(&event.reason),
                    source_updated_at,
                    observed_at,
                ],
            )
            .map_err(|e| Error::Store(format!("failed to write event: {e}")))?;

        Ok(Some(self.conn.last_insert_rowid()))
    }

    fn write_outbox(
        &mut self,
        event_id: EventRowId,
        _event: &OracleEvent,
        sink: &str,
        payload: &str,
    ) -> Result<()> {
        let oc = &self.outbox_columns;
        let sql = format!(
            "INSERT INTO {} ({}, {}, {}, {}, {}, {}) VALUES (?1, ?2, ?3, 'pending', 0, ?4)",
            self.outbox_table,
            oc.event_id,
            oc.sink,
            oc.payload,
            oc.status,
            oc.attempts,
            oc.next_attempt_at,
        );

        let now = fmt_ts(OffsetDateTime::now_utc())?;

        self.conn
            .execute(&sql, params![event_id, sink, payload, now])
            .map_err(|e| Error::Store(format!("failed to write outbox: {e}")))?;

        Ok(())
    }

    fn has_recent_event(
        &mut self,
        asset_id: &AssetId,
        provider: &ProviderId,
        event_type: &EventType,
        reason: &EventReason,
        within_secs: u64,
    ) -> Result<bool> {
        let ec = &self.event_columns;
        let sql = format!(
            "SELECT 1 FROM {} \
             WHERE {} = ?1 \
               AND {} = ?2 \
               AND {} = ?3 \
               AND {} = ?4 \
               AND {} > ?5 \
             LIMIT 1",
            self.events_table, ec.asset_id, ec.provider, ec.event_type, ec.reason, ec.observed_at,
        );

        let cutoff = OffsetDateTime::now_utc() - time::Duration::seconds(within_secs as i64);
        let cutoff_str = fmt_ts(cutoff)?;

        let result: Option<i64> = self
            .conn
            .query_row(
                &sql,
                params![
                    asset_id.as_str(),
                    provider.as_str(),
                    event_type.as_str(),
                    event_reason_as_str(reason),
                    cutoff_str,
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| Error::Store(format!("failed to query recent event: {e}")))?;

        Ok(result.is_some())
    }

    fn has_recent_disable_event(&mut self, asset_id: &AssetId) -> Result<bool> {
        let ec = &self.event_columns;
        let sql = format!(
            "SELECT {} FROM {} \
             WHERE {} = ?1 \
             ORDER BY {} DESC, {} DESC \
             LIMIT 1",
            ec.action, self.events_table, ec.asset_id, ec.observed_at, ec.id,
        );

        let result: Option<String> = self
            .conn
            .query_row(&sql, params![asset_id.as_str()], |row| row.get(0))
            .optional()
            .map_err(|e| Error::Store(format!("failed to query disable event: {e}")))?;

        Ok(result.as_deref() == Some("disable_asset"))
    }

    fn last_observed_rate(
        &mut self,
        asset_id: &AssetId,
        quote: &Quote,
        stale_after_secs: u64,
    ) -> Result<Option<RateRecord>> {
        // Query the rates table for the latest accepted rate (without provider filtering).
        let rates_record = self.last_accepted_rate(asset_id, quote)?;

        // Query the events table for the latest event with a candidate_rate.
        let ec = &self.event_columns;
        let events_sql = format!(
            "SELECT {}, {}, {}, {}, {}, {}, {} \
             FROM {} \
             WHERE {} = ?1 \
               AND {} = ?2 \
               AND {} IS NOT NULL \
             ORDER BY {} DESC, {} DESC \
             LIMIT 1",
            ec.chain_id,
            ec.symbol,
            ec.quote,
            ec.provider,
            ec.candidate_rate,
            ec.source_updated_at,
            ec.observed_at,
            self.events_table,
            ec.asset_id,
            ec.quote,
            ec.candidate_rate,
            ec.observed_at,
            ec.id,
        );

        let events_result: Option<Result<RateRecord>> = self
            .conn
            .query_row(
                &events_sql,
                params![asset_id.as_str(), quote.as_str()],
                |row| row_to_observed_rate_record(row, asset_id, stale_after_secs),
            )
            .optional()
            .map_err(|e| {
                Error::Store(format!(
                    "failed to query last observed rate from events: {e}"
                ))
            })?;

        let events_record = match events_result {
            Some(Ok(record)) => Some(record),
            Some(Err(e)) => return Err(e),
            None => None,
        };

        // Return whichever has the most recent observed_at.
        match (rates_record, events_record) {
            (Some(r), Some(e)) => {
                if r.observed_at >= e.observed_at {
                    Ok(Some(r))
                } else {
                    Ok(Some(e))
                }
            }
            (Some(r), None) => Ok(Some(r)),
            (None, Some(e)) => Ok(Some(e)),
            (None, None) => Ok(None),
        }
    }
}

impl OutboxStore for SqliteRateStore {
    fn pending_outbox(&mut self, now: OffsetDateTime, limit: usize) -> Result<Vec<OutboxDelivery>> {
        let oc = &self.outbox_columns;
        let sql = format!(
            r#"
            SELECT
                {id},
                {event_id},
                {sink},
                {payload},
                {attempts}
            FROM {table}
            WHERE {status} = 'pending'
              AND {next_attempt_at} <= ?1
            ORDER BY {next_attempt_at} ASC, {id} ASC
            LIMIT ?2
            "#,
            id = oc.id,
            event_id = oc.event_id,
            sink = oc.sink,
            payload = oc.payload,
            attempts = oc.attempts,
            table = self.outbox_table,
            status = oc.status,
            next_attempt_at = oc.next_attempt_at,
        );

        let now = fmt_ts(now)?;

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|err| Error::Store(format!("failed to prepare outbox query: {err}")))?;

        let rows = stmt
            .query_map(params![now, limit as i64], |row| {
                Ok(OutboxDelivery {
                    id: row.get(0)?,
                    event_id: row.get(1)?,
                    sink: row.get(2)?,
                    payload: row.get(3)?,
                    attempts: row.get::<_, i64>(4)? as u32,
                })
            })
            .map_err(|err| Error::Store(format!("failed to query SQLite outbox: {err}")))?;

        let mut deliveries = Vec::new();

        for row in rows {
            deliveries.push(
                row.map_err(|err| {
                    Error::Store(format!("failed to read SQLite outbox row: {err}"))
                })?,
            );
        }

        Ok(deliveries)
    }

    fn mark_outbox_delivered(&mut self, id: i64, delivered_at: OffsetDateTime) -> Result<()> {
        let oc = &self.outbox_columns;
        let sql = format!(
            r#"
            UPDATE {table}
            SET {status} = 'delivered',
                {delivered_at} = ?2,
                {last_error} = NULL
            WHERE {id_col} = ?1
            "#,
            table = self.outbox_table,
            status = oc.status,
            delivered_at = oc.delivered_at,
            last_error = oc.last_error,
            id_col = oc.id,
        );

        self.conn
            .execute(&sql, params![id, fmt_ts(delivered_at)?])
            .map_err(|err| {
                Error::Store(format!("failed to mark SQLite outbox delivered: {err}"))
            })?;

        Ok(())
    }

    fn mark_outbox_failed(
        &mut self,
        id: i64,
        attempts: u32,
        next_attempt_at: Option<OffsetDateTime>,
        last_error: &str,
    ) -> Result<()> {
        let oc = &self.outbox_columns;
        let status = if next_attempt_at.is_some() {
            "pending"
        } else {
            "dead"
        };

        let sql = format!(
            r#"
            UPDATE {table}
            SET {status_col} = ?2,
                {attempts_col} = ?3,
                {next_attempt_at_col} = ?4,
                {last_error_col} = ?5
            WHERE {id_col} = ?1
            "#,
            table = self.outbox_table,
            status_col = oc.status,
            attempts_col = oc.attempts,
            next_attempt_at_col = oc.next_attempt_at,
            last_error_col = oc.last_error,
            id_col = oc.id,
        );

        self.conn
            .execute(
                &sql,
                params![
                    id,
                    status,
                    attempts,
                    next_attempt_at.map_or_else(|| fmt_ts(OffsetDateTime::now_utc()), fmt_ts,)?,
                    last_error,
                ],
            )
            .map_err(|err| Error::Store(format!("failed to mark SQLite outbox failed: {err}")))?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SQL path helpers
// ---------------------------------------------------------------------------

fn sqlite_path_from_url(url: &str) -> Result<String> {
    // In-memory variants
    if url == "sqlite::memory:" || url == "sqlite://:memory:" {
        return Ok(":memory:".to_owned());
    }

    // File-based: strip sqlite:// prefix
    let path = url.strip_prefix("sqlite://").ok_or_else(|| {
        Error::Store(format!(
            "invalid sqlite url, expected sqlite:// prefix: {url}"
        ))
    })?;

    if path.is_empty() {
        return Err(Error::Store("sqlite path is empty after prefix".to_owned()));
    }

    Ok(path.to_owned())
}

/// SQL reserved words that should be rejected as identifiers.
const SQL_RESERVED_WORDS: &[&str] = &[
    "select",
    "insert",
    "update",
    "delete",
    "create",
    "drop",
    "alter",
    "table",
    "index",
    "view",
    "trigger",
    "where",
    "from",
    "join",
    "on",
    "and",
    "or",
    "not",
    "null",
    "is",
    "in",
    "like",
    "between",
    "as",
    "order",
    "group",
    "having",
    "limit",
    "offset",
    "union",
    "except",
    "intersect",
    "into",
    "values",
    "set",
    "primary",
    "key",
    "foreign",
    "references",
    "check",
    "default",
    "constraint",
    "unique",
    "cascade",
    "restrict",
    "if",
    "exists",
    "case",
    "when",
    "then",
    "else",
    "end",
    "begin",
    "commit",
    "rollback",
    "transaction",
    "true",
    "false",
    "unknown",
];

fn validate_identifier(value: &str, field: &str) -> Result<String> {
    if value.is_empty() {
        return Err(Error::Store(format!("{field} must not be empty")));
    }
    let first = value.as_bytes()[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return Err(Error::Store(format!(
            "{field} must start with a letter or underscore, got: {value}"
        )));
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(Error::Store(format!(
            "{field} contains invalid characters, only [A-Za-z0-9_] allowed: {value}"
        )));
    }
    if SQL_RESERVED_WORDS.contains(&value.to_ascii_lowercase().as_str()) {
        return Err(Error::Store(format!(
            "{field} is a SQL reserved word and cannot be used as an identifier: {value}"
        )));
    }
    Ok(value.to_owned())
}

// ---------------------------------------------------------------------------
// Timestamp helpers
// ---------------------------------------------------------------------------

fn fmt_ts(value: OffsetDateTime) -> Result<String> {
    value
        .format(&Rfc3339)
        .map_err(|e| Error::Store(format!("failed to format timestamp: {e}")))
}

fn fmt_ts_opt(value: Option<OffsetDateTime>) -> Result<Option<String>> {
    value.map(fmt_ts).transpose()
}

fn parse_ts(value: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|e| Error::Store(format!("failed to parse timestamp '{value}': {e}")))
}

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

fn row_to_rate_record(row: &rusqlite::Row) -> rusqlite::Result<Result<RateRecord>> {
    let asset_id: String = row.get(0)?;
    let chain_id: String = row.get(1)?;
    let caip2: String = row.get(2)?;
    let symbol: String = row.get(3)?;
    let quote: String = row.get(4)?;
    let provider: String = row.get(5)?;
    let rate: String = row.get(6)?;
    let source_updated_at: Option<String> = row.get(7)?;
    let observed_at: String = row.get(8)?;
    let expires_at: String = row.get(9)?;

    Ok(build_rate_record(
        &asset_id,
        &chain_id,
        &caip2,
        &symbol,
        &quote,
        &provider,
        &rate,
        source_updated_at.as_deref(),
        &observed_at,
        &expires_at,
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_rate_record(
    asset_id: &str,
    chain_id: &str,
    caip2: &str,
    symbol: &str,
    quote: &str,
    provider: &str,
    rate: &str,
    source_updated_at: Option<&str>,
    observed_at: &str,
    expires_at: &str,
) -> Result<RateRecord> {
    let source_updated_at = source_updated_at.map(parse_ts).transpose()?;

    Ok(RateRecord {
        asset_id: AssetId::new(asset_id)?,
        chain_id: ChainId::new(chain_id)?,
        caip2: caip2.to_owned(),
        symbol: symbol.to_owned(),
        quote: Quote::new(quote)?,
        provider: ProviderId::new(provider)?,
        rate: RateAmount::parse(rate)?,
        source_updated_at,
        observed_at: parse_ts(observed_at)?,
        expires_at: parse_ts(expires_at)?,
    })
}

/// Map an events-table row to a [`RateRecord`], filling missing fields with
/// sensible defaults. Uses the configured `stale_after_secs` for expiry.
fn row_to_observed_rate_record(
    row: &rusqlite::Row,
    asset_id: &AssetId,
    stale_after_secs: u64,
) -> rusqlite::Result<Result<RateRecord>> {
    let chain_id: Option<String> = row.get(0)?;
    let symbol: String = row.get(1)?;
    let quote_str: String = row.get(2)?;
    let provider: String = row.get(3)?;
    let candidate_rate: String = row.get(4)?;
    let source_updated_at: Option<String> = row.get(5)?;
    let observed_at: String = row.get(6)?;

    // Fall back to asset_id when chain_id is null or empty.
    let chain_id = chain_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| asset_id.as_str().to_owned());

    // Compute expires_at as observed_at + configured stale_after_secs.
    let expires_at = parse_ts(&observed_at)
        .and_then(|obs| fmt_ts(obs + time::Duration::seconds(stale_after_secs as i64)));

    let expires_at = match expires_at {
        Ok(s) => s,
        Err(e) => return Ok(Err(e)),
    };

    Ok(build_rate_record(
        asset_id.as_str(),
        &chain_id,
        "",
        &symbol,
        &quote_str,
        &provider,
        &candidate_rate,
        source_updated_at.as_deref(),
        &observed_at,
        &expires_at,
    ))
}

// ---------------------------------------------------------------------------
// Enum string helpers
// ---------------------------------------------------------------------------

fn event_action_as_str(value: &EventAction) -> &'static str {
    value.as_str()
}

fn event_reason_as_str(value: &EventReason) -> &'static str {
    value.as_str()
}
