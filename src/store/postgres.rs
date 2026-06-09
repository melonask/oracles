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
#[cfg(feature = "postgres-tls")]
use postgres::Client;
use postgres::Row;
#[cfg(not(feature = "postgres-tls"))]
use postgres::{Client, NoTls};
#[cfg(feature = "postgres-tls")]
use postgres_native_tls::MakeTlsConnector;
use time::OffsetDateTime;

/// A PostgreSQL-backed rate and event store.
pub struct PostgresRateStore {
    client: Client,
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

impl PostgresRateStore {
    /// Open a PostgreSQL store from the resolved configuration.
    ///
    /// Looks up the store config by name, validates the driver and write mode,
    /// opens a connection, and optionally runs schema migration.
    pub fn open(config: &ResolvedConfig) -> Result<Self> {
        let store_config = config
            .stores
            .get(&config.oracles.store)
            .ok_or_else(|| Error::Store(format!("store not found: {}", config.oracles.store)))?;

        if store_config.driver != StoreDriver::Postgres {
            return Err(Error::Store("store driver must be Postgres".to_owned()));
        }

        // PostgreSQL only supports a single connection until connection
        // pooling is implemented. Reject any other value upfront so it is not
        // silently ignored.
        if store_config.max_connections != 1 {
            return Err(Error::Store(format!(
                "PostgreSQL store currently only supports max_connections = 1. \
                 Got: {}. Connection pooling is not yet implemented.",
                store_config.max_connections
            )));
        }

        let write_mode = config.oracles.table.write_mode.clone();
        let stale_after_secs = config.oracles.stale_after_secs;

        let rates_table = validate_identifier(&config.oracles.table.name, "oracles.table.name")?;
        let events_table = validate_identifier(&config.events.table, "events.table")?;
        let outbox_table = validate_identifier(&config.outbox.table, "outbox.table")?;

        // Extract column name mappings from the resolved config.
        let rate_columns = config.oracles.table.columns.clone();
        let event_columns = config.events.columns.clone();
        let outbox_columns = config.outbox.columns.clone();

        // Append connect_timeout to the URL if not already present.
        let url = if store_config.url.contains("connect_timeout=") {
            store_config.url.clone()
        } else if store_config.url.contains('?') {
            format!(
                "{}&connect_timeout={}",
                store_config.url, store_config.connect_timeout_secs
            )
        } else {
            format!(
                "{}?connect_timeout={}",
                store_config.url, store_config.connect_timeout_secs
            )
        };

        let client = {
            #[cfg(feature = "postgres-tls")]
            {
                let connector = native_tls::TlsConnector::new()
                    .map_err(|e| Error::Store(format!("failed to create TLS connector: {e}")))?;
                let tls = MakeTlsConnector::new(connector);
                Client::connect(&url, tls).map_err(|e| {
                    Error::Store(format!("failed to connect to postgres with TLS: {e}"))
                })?
            }
            #[cfg(not(feature = "postgres-tls"))]
            {
                Client::connect(&url, NoTls)
                    .map_err(|e| Error::Store(format!("failed to connect to postgres: {e}")))?
            }
        };

        let mut store = Self {
            client,
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

    /// Run schema migration: create tables and indexes if they do not exist.
    fn migrate(&mut self) -> Result<()> {
        let rc = &self.rate_columns;
        let ec = &self.event_columns;
        let oc = &self.outbox_columns;

        // -- rates table --
        let create_rates = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                {} BIGSERIAL PRIMARY KEY,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TIMESTAMPTZ,
                {} TIMESTAMPTZ NOT NULL,
                {} TIMESTAMPTZ NOT NULL
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
        self.client
            .execute(&create_rates, &[])
            .map_err(|e| Error::Store(format!("failed to create rates table: {e}")))?;

        let rates_asset_quote_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_asset_quote_idx ON {t} ({asset_id}, {quote})",
            t = self.rates_table,
            asset_id = rc.asset_id,
            quote = rc.quote,
        );
        self.client
            .execute(&rates_asset_quote_idx, &[])
            .map_err(|e| Error::Store(format!("failed to create rates asset_quote index: {e}")))?;

        let rates_expires_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_expires_at_idx ON {t} ({expires_at})",
            t = self.rates_table,
            expires_at = rc.expires_at,
        );
        self.client
            .execute(&rates_expires_idx, &[])
            .map_err(|e| Error::Store(format!("failed to create rates expires_at index: {e}")))?;

        // Unique constraint for upsert mode only.
        // In append mode, multiple rows per (asset_id, quote, provider) are allowed.
        if self.write_mode == WriteMode::Upsert {
            let rates_unique_idx = format!(
                "CREATE UNIQUE INDEX IF NOT EXISTS {t}_asset_quote_provider_uniq ON {t} ({asset_id}, {quote}, {provider})",
                t = self.rates_table,
                asset_id = rc.asset_id,
                quote = rc.quote,
                provider = rc.provider,
            );
            self.client
                .execute(&rates_unique_idx, &[])
                .map_err(|e| Error::Store(format!("failed to create rates unique index: {e}")))?;
        }

        // -- events table --
        let create_events = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                {} BIGSERIAL PRIMARY KEY,
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
                {} TIMESTAMPTZ,
                {} TIMESTAMPTZ NOT NULL
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
        self.client
            .execute(&create_events, &[])
            .map_err(|e| Error::Store(format!("failed to create events table: {e}")))?;

        let events_asset_quote_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_asset_quote_idx ON {t} ({asset_id}, {quote})",
            t = self.events_table,
            asset_id = ec.asset_id,
            quote = ec.quote,
        );
        self.client
            .execute(&events_asset_quote_idx, &[])
            .map_err(|e| Error::Store(format!("failed to create events asset_quote index: {e}")))?;

        let events_observed_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_observed_at_idx ON {t} ({observed_at})",
            t = self.events_table,
            observed_at = ec.observed_at,
        );
        self.client
            .execute(&events_observed_idx, &[])
            .map_err(|e| Error::Store(format!("failed to create events observed_at index: {e}")))?;

        // -- outbox table --
        let create_outbox = format!(
            "CREATE TABLE IF NOT EXISTS {} (
                {} BIGSERIAL PRIMARY KEY,
                {} BIGINT,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL,
                {} TEXT NOT NULL DEFAULT 'pending',
                {} INTEGER NOT NULL DEFAULT 0,
                {} TIMESTAMPTZ NOT NULL,
                {} TIMESTAMPTZ,
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
        self.client
            .execute(&create_outbox, &[])
            .map_err(|e| Error::Store(format!("failed to create outbox table: {e}")))?;

        let outbox_status_idx = format!(
            "CREATE INDEX IF NOT EXISTS {t}_status_next_idx ON {t} ({status}, {next_attempt_at})",
            t = self.outbox_table,
            status = oc.status,
            next_attempt_at = oc.next_attempt_at,
        );
        self.client
            .execute(&outbox_status_idx, &[])
            .map_err(|e| Error::Store(format!("failed to create outbox status_next index: {e}")))?;

        Ok(())
    }
}

impl RateStore for PostgresRateStore {
    fn begin_decision(&mut self) -> Result<()> {
        if self.in_tx {
            return Err(Error::Store("transaction already in progress".to_owned()));
        }
        self.client
            .execute("BEGIN", &[])
            .map_err(|e| Error::Store(format!("failed to begin transaction: {e}")))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit_decision(&mut self) -> Result<()> {
        self.client
            .execute("COMMIT", &[])
            .map_err(|e| Error::Store(format!("failed to commit transaction: {e}")))?;
        self.in_tx = false;
        Ok(())
    }

    fn rollback_decision(&mut self) -> Result<()> {
        self.client
            .execute("ROLLBACK", &[])
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
             WHERE {} = $1 AND {} = $2 \
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

        let result = self
            .client
            .query_opt(&sql, &[&asset_id.as_str(), &quote.as_str()])
            .map_err(|e| Error::Store(format!("failed to query last accepted rate: {e}")))?;

        match result {
            Some(row) => row_to_rate_record(&row),
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
             WHERE {} = $1 AND {} = $2 AND {} = $3 \
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

        let result = self
            .client
            .query_opt(
                &sql,
                &[&asset_id.as_str(), &quote.as_str(), &provider.as_str()],
            )
            .map_err(|e| {
                Error::Store(format!(
                    "failed to query last accepted rate for provider: {e}"
                ))
            })?;

        match result {
            Some(row) => row_to_rate_record(&row),
            None => Ok(None),
        }
    }

    fn write_accepted_rate(&mut self, record: &RateRecord) -> Result<()> {
        let c = &self.rate_columns;

        match self.write_mode {
            WriteMode::Upsert => {
                // Use proper atomic upsert with ON CONFLICT.
                let insert_sql = format!(
                    "INSERT INTO {} \
                     ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
                     ON CONFLICT ({}, {}, {}) DO UPDATE SET \
                     {} = EXCLUDED.{}, \
                     {} = EXCLUDED.{}, \
                     {} = EXCLUDED.{}, \
                     {} = EXCLUDED.{}, \
                     {} = EXCLUDED.{}, \
                     {} = EXCLUDED.{}, \
                     {} = EXCLUDED.{}",
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
                    c.asset_id,
                    c.quote,
                    c.provider,
                    c.chain_id,
                    c.chain_id,
                    c.caip2,
                    c.caip2,
                    c.symbol,
                    c.symbol,
                    c.rate,
                    c.rate,
                    c.source_updated_at,
                    c.source_updated_at,
                    c.observed_at,
                    c.observed_at,
                    c.expires_at,
                    c.expires_at,
                );
                self.client
                    .execute(
                        &insert_sql,
                        &[
                            &record.asset_id.as_str(),
                            &record.chain_id.as_str(),
                            &record.caip2.as_str(),
                            &record.symbol.as_str(),
                            &record.quote.as_str(),
                            &record.provider.as_str(),
                            &record.rate.to_string(),
                            &record.source_updated_at,
                            &record.observed_at,
                            &record.expires_at,
                        ],
                    )
                    .map_err(|e| Error::Store(format!("failed to write accepted rate: {e}")))?;
            }
            WriteMode::Append => {
                let insert_sql = format!(
                    "INSERT INTO {} \
                     ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
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
                self.client
                    .execute(
                        &insert_sql,
                        &[
                            &record.asset_id.as_str(),
                            &record.chain_id.as_str(),
                            &record.caip2.as_str(),
                            &record.symbol.as_str(),
                            &record.quote.as_str(),
                            &record.provider.as_str(),
                            &record.rate.to_string(),
                            &record.source_updated_at,
                            &record.observed_at,
                            &record.expires_at,
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
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
             RETURNING {}",
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
            ec.id,
        );

        let chain_id: Option<&str> = event.chain_id.as_ref().map(|c| c.as_str());
        let previous_rate = event.previous_rate.as_ref().map(|r| r.to_string());
        let candidate_rate = event.candidate_rate.as_ref().map(|r| r.to_string());
        let change_pct = event.change_pct.map(|d| d.to_string());

        let row = self
            .client
            .query_one(
                &sql,
                &[
                    &event.event_type.as_str(),
                    &event.asset_id.as_str(),
                    &chain_id,
                    &event.symbol.as_str(),
                    &event.quote.as_str(),
                    &event.provider.as_str(),
                    &previous_rate,
                    &candidate_rate,
                    &change_pct,
                    &event_action_as_str(&event.action),
                    &event_reason_as_str(&event.reason),
                    &event.source_updated_at,
                    &event.observed_at,
                ],
            )
            .map_err(|e| Error::Store(format!("failed to write event: {e}")))?;

        let id: i64 = row
            .try_get(0)
            .map_err(|e| Error::Store(format!("failed to read returned event id: {e}")))?;

        Ok(Some(id))
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
            "INSERT INTO {} ({}, {}, {}, {}, {}, {}) \
             VALUES ($1, $2, $3, 'pending', 0, $4)",
            self.outbox_table,
            oc.event_id,
            oc.sink,
            oc.payload,
            oc.status,
            oc.attempts,
            oc.next_attempt_at,
        );

        let now = OffsetDateTime::now_utc();

        self.client
            .execute(&sql, &[&event_id, &sink, &payload, &now])
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
             WHERE {} = $1 \
               AND {} = $2 \
               AND {} = $3 \
               AND {} = $4 \
               AND {} > $5 \
             LIMIT 1",
            self.events_table, ec.asset_id, ec.provider, ec.event_type, ec.reason, ec.observed_at,
        );

        let cutoff = OffsetDateTime::now_utc() - time::Duration::seconds(within_secs as i64);

        let result = self
            .client
            .query_opt(
                &sql,
                &[
                    &asset_id.as_str(),
                    &provider.as_str(),
                    &event_type.as_str(),
                    &event_reason_as_str(reason),
                    &cutoff,
                ],
            )
            .map_err(|e| Error::Store(format!("failed to query recent event: {e}")))?;

        Ok(result.is_some())
    }

    fn has_recent_disable_event(&mut self, asset_id: &AssetId) -> Result<bool> {
        let ec = &self.event_columns;
        let sql = format!(
            "SELECT {} FROM {} \
             WHERE {} = $1 \
             ORDER BY {} DESC, {} DESC \
             LIMIT 1",
            ec.action, self.events_table, ec.asset_id, ec.observed_at, ec.id,
        );

        let result = self
            .client
            .query_opt(&sql, &[&asset_id.as_str()])
            .map_err(|e| Error::Store(format!("failed to query disable event: {e}")))?;

        match result {
            Some(row) => {
                let action: String = row
                    .try_get(0)
                    .map_err(|e| Error::Store(format!("failed to read action: {e}")))?;
                Ok(action == "disable_asset")
            }
            None => Ok(false),
        }
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
             WHERE {} = $1 \
               AND {} = $2 \
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

        let result = self
            .client
            .query_opt(&events_sql, &[&asset_id.as_str(), &quote.as_str()])
            .map_err(|e| {
                Error::Store(format!(
                    "failed to query last observed rate from events: {e}"
                ))
            })?;

        let events_record = match result {
            Some(row) => row_to_observed_rate_record(&row, asset_id, stale_after_secs)?,
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

impl OutboxStore for PostgresRateStore {
    fn pending_outbox(&mut self, now: OffsetDateTime, limit: usize) -> Result<Vec<OutboxDelivery>> {
        let oc = &self.outbox_columns;
        let id_col = &oc.id;
        let event_id_col = &oc.event_id;
        let sink_col = &oc.sink;
        let payload_col = &oc.payload;
        let attempts_col = &oc.attempts;
        let status_col = &oc.status;
        let next_attempt_col = &oc.next_attempt_at;
        let table = &self.outbox_table;

        let sql = format!(
            "SELECT {id_col}, {event_id_col}, {sink_col}, {payload_col}, {attempts_col} \
             FROM {table} \
             WHERE {status_col} = 'pending' \
               AND {next_attempt_col} <= $1 \
             ORDER BY {next_attempt_col} ASC, {id_col} ASC \
             LIMIT $2",
        );

        let limit_i64 = i64::try_from(limit)
            .map_err(|_| Error::Store("outbox limit out of range for i64".to_owned()))?;

        let rows = self
            .client
            .query(&sql, &[&now, &limit_i64])
            .map_err(|e| Error::Store(format!("failed to query postgres outbox: {e}")))?;

        let mut deliveries = Vec::new();
        for row in rows {
            let id: i64 = row
                .try_get(0)
                .map_err(|e| Error::Store(format!("failed to read outbox id: {e}")))?;
            let event_id: Option<i64> = row
                .try_get(1)
                .map_err(|e| Error::Store(format!("failed to read outbox event_id: {e}")))?;
            let sink: String = row
                .try_get(2)
                .map_err(|e| Error::Store(format!("failed to read outbox sink: {e}")))?;
            let payload: String = row
                .try_get(3)
                .map_err(|e| Error::Store(format!("failed to read outbox payload: {e}")))?;
            let attempts: i32 = row
                .try_get(4)
                .map_err(|e| Error::Store(format!("failed to read outbox attempts: {e}")))?;

            deliveries.push(OutboxDelivery {
                id,
                event_id,
                sink,
                payload,
                attempts: attempts as u32,
            });
        }

        Ok(deliveries)
    }

    fn mark_outbox_delivered(&mut self, id: i64, delivered_at: OffsetDateTime) -> Result<()> {
        let oc = &self.outbox_columns;
        let sql = format!(
            "UPDATE {table} \
             SET {} = 'delivered', \
                 {} = $2, \
                 {} = NULL \
             WHERE {} = $1",
            oc.status,
            oc.delivered_at,
            oc.last_error,
            oc.id,
            table = self.outbox_table
        );

        self.client
            .execute(&sql, &[&id, &delivered_at])
            .map_err(|e| Error::Store(format!("failed to mark postgres outbox delivered: {e}")))?;

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
            "UPDATE {table} \
             SET {} = $2, \
                 {} = $3, \
                 {} = $4, \
                 {} = $5 \
             WHERE {} = $1",
            oc.status,
            oc.attempts,
            oc.next_attempt_at,
            oc.last_error,
            oc.id,
            table = self.outbox_table
        );

        let attempts_i32 = i32::try_from(attempts)
            .map_err(|_| Error::Store("attempts out of range for i32".to_owned()))?;

        self.client
            .execute(
                &sql,
                &[
                    &id,
                    &status,
                    &attempts_i32,
                    &next_attempt_at.unwrap_or_else(OffsetDateTime::now_utc),
                    &last_error,
                ],
            )
            .map_err(|e| Error::Store(format!("failed to mark postgres outbox failed: {e}")))?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Identifier validation
// ---------------------------------------------------------------------------

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
// Row mapping
// ---------------------------------------------------------------------------

fn row_to_rate_record(row: &Row) -> Result<Option<RateRecord>> {
    let asset_id: String = row
        .try_get(0)
        .map_err(|e| Error::Store(format!("failed to read asset_id: {e}")))?;
    let chain_id: String = row
        .try_get(1)
        .map_err(|e| Error::Store(format!("failed to read chain_id: {e}")))?;
    let caip2: String = row
        .try_get(2)
        .map_err(|e| Error::Store(format!("failed to read caip2: {e}")))?;
    let symbol: String = row
        .try_get(3)
        .map_err(|e| Error::Store(format!("failed to read symbol: {e}")))?;
    let quote: String = row
        .try_get(4)
        .map_err(|e| Error::Store(format!("failed to read quote: {e}")))?;
    let provider: String = row
        .try_get(5)
        .map_err(|e| Error::Store(format!("failed to read provider: {e}")))?;
    let rate: String = row
        .try_get(6)
        .map_err(|e| Error::Store(format!("failed to read rate: {e}")))?;
    let source_updated_at: Option<OffsetDateTime> = row
        .try_get(7)
        .map_err(|e| Error::Store(format!("failed to read source_updated_at: {e}")))?;
    let observed_at: OffsetDateTime = row
        .try_get(8)
        .map_err(|e| Error::Store(format!("failed to read observed_at: {e}")))?;
    let expires_at: OffsetDateTime = row
        .try_get(9)
        .map_err(|e| Error::Store(format!("failed to read expires_at: {e}")))?;

    Ok(Some(RateRecord {
        asset_id: AssetId::new(asset_id)?,
        chain_id: ChainId::new(chain_id)?,
        caip2,
        symbol,
        quote: Quote::new(quote)?,
        provider: ProviderId::new(provider)?,
        rate: RateAmount::parse(&rate)?,
        source_updated_at,
        observed_at,
        expires_at,
    }))
}

/// Map an events-table row to a [`RateRecord`], filling missing fields with
/// sensible defaults.
fn row_to_observed_rate_record(
    row: &Row,
    asset_id: &AssetId,
    stale_after_secs: u64,
) -> Result<Option<RateRecord>> {
    let chain_id: Option<String> = row
        .try_get(0)
        .map_err(|e| Error::Store(format!("failed to read chain_id: {e}")))?;
    let symbol: String = row
        .try_get(1)
        .map_err(|e| Error::Store(format!("failed to read symbol: {e}")))?;
    let quote: String = row
        .try_get(2)
        .map_err(|e| Error::Store(format!("failed to read quote: {e}")))?;
    let provider: String = row
        .try_get(3)
        .map_err(|e| Error::Store(format!("failed to read provider: {e}")))?;
    let candidate_rate: String = row
        .try_get(4)
        .map_err(|e| Error::Store(format!("failed to read candidate_rate: {e}")))?;
    let source_updated_at: Option<OffsetDateTime> = row
        .try_get(5)
        .map_err(|e| Error::Store(format!("failed to read source_updated_at: {e}")))?;
    let observed_at: OffsetDateTime = row
        .try_get(6)
        .map_err(|e| Error::Store(format!("failed to read observed_at: {e}")))?;

    // Fall back to asset_id when chain_id is null or empty.
    let chain_id = chain_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| asset_id.as_str().to_owned());

    Ok(Some(RateRecord {
        asset_id: AssetId::new(asset_id.as_str())?,
        chain_id: ChainId::new(chain_id)?,
        caip2: String::new(),
        symbol,
        quote: Quote::new(quote)?,
        provider: ProviderId::new(provider)?,
        rate: RateAmount::parse(&candidate_rate)?,
        source_updated_at,
        observed_at,
        expires_at: observed_at + time::Duration::seconds(stale_after_secs as i64),
    }))
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
