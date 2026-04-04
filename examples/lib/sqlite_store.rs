#![allow(dead_code)]

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use mini_cqrs_es::{CqrsError, Event, EventPayload, EventStore, Uuid};

/// A transparent wrapper around `serde_json::Value` that implements `EventPayload`.
/// Used to reconstruct `Event` from stored data without knowing the concrete event type.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
struct RawPayload(serde_json::Value);

impl fmt::Display for RawPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RawPayload")
    }
}

impl EventPayload for RawPayload {}

/// An event store backed by SQLite via sqlx.
#[derive(Clone)]
pub struct SqliteEventStore {
    pool: SqlitePool,
}

impl SqliteEventStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS events (
                id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                aggregate_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                version INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                PRIMARY KEY (aggregate_id, version)
            )",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

impl EventStore for SqliteEventStore {
    async fn save_events(
        &self,
        aggregate_id: Uuid,
        events: &[Event],
        expected_version: u64,
    ) -> Result<(), CqrsError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CqrsError::EventStore(e.to_string()))?;

        // Check optimistic concurrency
        let agg_id_str = aggregate_id.to_string();
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(version), 0) FROM events WHERE aggregate_id = ?",
        )
        .bind(&agg_id_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CqrsError::EventStore(e.to_string()))?;

        let actual_version = row.0 as u64;
        if actual_version != expected_version {
            return Err(CqrsError::Conflict {
                expected_version,
                actual_version,
            });
        }

        // Insert events
        for event in events {
            let payload_json = serde_json::to_string(
                &event.get_payload::<RawPayload>().map_err(|e| {
                    CqrsError::EventStore(format!("Failed to read event payload: {}", e))
                })?,
            )
            .map_err(|e| CqrsError::EventStore(e.to_string()))?;

            sqlx::query(
                "INSERT INTO events (id, event_type, aggregate_id, payload, version, timestamp) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&event.id)
            .bind(&event.event_type)
            .bind(&agg_id_str)
            .bind(&payload_json)
            .bind(event.version as i64)
            .bind(event.timestamp.to_rfc3339())
            .execute(&mut *tx)
            .await
            .map_err(|e| CqrsError::EventStore(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| CqrsError::EventStore(e.to_string()))?;
        Ok(())
    }

    async fn load_events(&self, aggregate_id: Uuid) -> Result<(Vec<Event>, u64), CqrsError> {
        let agg_id_str = aggregate_id.to_string();

        let rows: Vec<(String, String, String, String, i64, String)> = sqlx::query_as(
            "SELECT id, event_type, aggregate_id, payload, version, timestamp
             FROM events
             WHERE aggregate_id = ?
             ORDER BY version ASC",
        )
        .bind(&agg_id_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CqrsError::EventStore(e.to_string()))?;

        let mut events = Vec::with_capacity(rows.len());
        let mut max_version: u64 = 0;

        for (id, event_type, _agg_id, payload_str, version, timestamp_str) in rows {
            let version = version as u64;
            if version > max_version {
                max_version = version;
            }

            let payload_value: serde_json::Value =
                serde_json::from_str(&payload_str).map_err(|e| {
                    CqrsError::EventStore(format!("Failed to parse payload JSON: {}", e))
                })?;

            // Reconstruct Event using RawPayload (transparent serde)
            let mut event =
                Event::new(aggregate_id, RawPayload(payload_value), version).map_err(|e| {
                    CqrsError::EventStore(format!("Failed to reconstruct event: {}", e))
                })?;

            // Overwrite public fields with stored values
            event.id = id;
            event.event_type = event_type;
            event.timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            events.push(event);
        }

        Ok((events, max_version))
    }
}
