#![allow(dead_code)]

use chrono::Utc;
use sqlx::SqlitePool;

use mini_cqrs_es::{CqrsError, EventMetadata, EventStore, NewEvent, StoredEvent};

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
                aggregate_type TEXT NOT NULL,
                event_type TEXT NOT NULL,
                aggregate_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                metadata TEXT NOT NULL,
                version INTEGER NOT NULL,
                global_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                UNIQUE (aggregate_type, aggregate_id, version)
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
        aggregate_type: &str,
        aggregate_id: &str,
        events: &[NewEvent],
        expected_version: u64,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CqrsError::EventStore(e.to_string()))?;

        // Check optimistic concurrency
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(version), 0) FROM events WHERE aggregate_type = ? AND aggregate_id = ?",
        )
        .bind(aggregate_type)
        .bind(aggregate_id)
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

        let mut persisted = Vec::with_capacity(events.len());
        for (i, event) in events.iter().enumerate() {
            let payload_json =
                serde_json::to_string(&event.payload).map_err(|e| CqrsError::EventStore(e.to_string()))?;
            let metadata_json = serde_json::to_string(&event.metadata)
                .map_err(|e| CqrsError::EventStore(e.to_string()))?;
            let version = actual_version + i as u64 + 1;
            let id = format!("{aggregate_type}-{aggregate_id}-{version}");

            let seq: i64 = sqlx::query_scalar(
                "INSERT INTO events (id, aggregate_type, event_type, aggregate_id, payload, metadata, version, timestamp)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                 RETURNING global_sequence",
            )
            .bind(&id)
            .bind(aggregate_type)
            .bind(&event.event_type)
            .bind(aggregate_id)
            .bind(&payload_json)
            .bind(&metadata_json)
            .bind(version as i64)
            .bind(event.timestamp.to_rfc3339())
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| CqrsError::EventStore(e.to_string()))?;

            persisted.push(StoredEvent {
                id,
                aggregate_id: aggregate_id.to_string(),
                aggregate_type: aggregate_type.to_string(),
                version,
                event_type: event.event_type.clone(),
                payload: event.payload.clone(),
                metadata: event.metadata.clone(),
                global_sequence: Some(seq),
                timestamp: event.timestamp,
            });
        }

        tx.commit()
            .await
            .map_err(|e| CqrsError::EventStore(e.to_string()))?;
        Ok(persisted)
    }

    async fn load_events(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<(Vec<StoredEvent>, u64), CqrsError> {
        let rows: Vec<(String, String, String, String, String, String, i64, i64, String)> =
            sqlx::query_as(
                "SELECT id, aggregate_type, event_type, aggregate_id, payload, metadata, version, global_sequence, timestamp
             FROM events
             WHERE aggregate_type = ? AND aggregate_id = ?
             ORDER BY version ASC",
            )
            .bind(aggregate_type)
            .bind(aggregate_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| CqrsError::EventStore(e.to_string()))?;

        let mut events = Vec::with_capacity(rows.len());
        let mut max_version: u64 = 0;

        for (
            id,
            row_aggregate_type,
            event_type,
            row_aggregate_id,
            payload_str,
            metadata_str,
            version,
            global_sequence,
            timestamp_str,
        ) in rows
        {
            let version = version as u64;
            if version > max_version {
                max_version = version;
            }

            let payload_value: serde_json::Value = serde_json::from_str(&payload_str)
                .map_err(|e| CqrsError::EventStore(format!("Failed to parse payload JSON: {}", e)))?;
            let metadata: EventMetadata = serde_json::from_str(&metadata_str)
                .map_err(|e| CqrsError::EventStore(format!("Failed to parse metadata JSON: {}", e)))?;

            events.push(StoredEvent {
                id,
                aggregate_id: row_aggregate_id,
                aggregate_type: row_aggregate_type,
                version,
                event_type,
                payload: payload_value,
                metadata,
                global_sequence: Some(global_sequence),
                timestamp: chrono::DateTime::parse_from_rfc3339(&timestamp_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            });
        }

        Ok((events, max_version))
    }
}
