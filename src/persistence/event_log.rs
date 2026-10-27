//! Toasty-backed infrastructure, separate from the domain traits.
//!
//! Select a Toasty driver through explicit Cargo features and connection URLs.
//! Schema creation is a separate operation on a fresh, provisioned database.

use std::num::NonZeroUsize;
use std::time::Duration;

use toasty::{Executor, schema::Model};

use crate::{CqrsError, EventStore, NewEvent, StoredEvent, StreamKey};

use super::{SnapshotCache, SnapshotRecord};

/// Infrastructure model for the append-only event log.
///
/// Register this model in the durable event database, separately from disposable
/// read models. Write events through [`EventLog`]; generated update,
/// delete, and upsert methods bypass its append-only and concurrency contracts.
/// JSON and UTC RFC3339 timestamps use text columns for backend portability.
#[derive(Debug, toasty::Model)]
#[table = "cqrs_events"]
#[unique(aggregate_type, aggregate_id, version)]
pub struct EventRecord {
    #[key]
    pub id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub version: i64,
    pub event_type: String,
    pub schema_version: i64,
    pub payload: String,
    pub metadata: String,
    pub timestamp: String,
}

impl EventRecord {
    fn into_event(self) -> Result<StoredEvent, CqrsError> {
        let version = positive_version(self.version)?;
        Ok(StoredEvent {
            id: self.id,
            aggregate_type: self.aggregate_type,
            aggregate_id: self.aggregate_id,
            version,
            event_type: self.event_type,
            schema_version: u32::try_from(self.schema_version)
                .ok()
                .filter(|v| *v > 0)
                .ok_or_else(|| CqrsError::EventFormat("invalid stored schema revision".into()))?,
            payload: serde_json::from_str(&self.payload)?,
            metadata: serde_json::from_str(&self.metadata)?,
            global_sequence: None,
            timestamp: chrono::DateTime::parse_from_rfc3339(&self.timestamp)
                .map_err(|err| CqrsError::EventStore(format!("invalid event timestamp: {err}")))?
                .with_timezone(&chrono::Utc),
        })
    }
}

/// An event store backed by Toasty models and database transactions.
///
/// Stream versions are limited to `i64::MAX`. Every append, including an empty
/// append, checks the expected version. Returned envelopes are committed and
/// ordered; `global_sequence` is always `None`.
///
/// Known serialization failures are retried at most eight times with bounded
/// backoff and the original expected version. Other database errors are returned
/// as storage errors. In particular, opaque uniqueness errors can remain storage
/// errors on PostgreSQL; contention does not always produce `Conflict`.
/// Transport failures are never automatically retried because commit may already
/// have succeeded. Consumers and snapshots are outside this store's transaction.
#[derive(Clone)]
pub struct EventLog {
    database: toasty::Db,
}

impl EventLog {
    /// Uses an application-configured database that includes [`EventRecord`].
    /// This neither creates nor changes the schema.
    pub fn new(database: toasty::Db) -> Self {
        Self { database }
    }

    /// Connects with event and snapshot-support models registered, without DDL.
    pub async fn connect(url: &str) -> toasty::Result<Self> {
        let database = toasty::Db::builder()
            .models(Self::models())
            .connect(url)
            .await?;
        Ok(Self::new(database))
    }

    /// Creates framework tables in a fresh, provisioned database. This is an
    /// explicit setup operation, not a reset or a migration of existing data.
    pub async fn create_schema(&self) -> Result<(), CqrsError> {
        self.database
            .push_schema()
            .await
            .map_err(|error| CqrsError::storage("create event schema", error))
    }

    /// Returns event and snapshot-support models. Register other support models with
    /// [`toasty::schema::Model::register`] before passing the set to a database builder.
    /// Keep disposable read models in a separate database/model set.
    /// This also works when Toasty is accessed exclusively through our re-export.
    pub fn models() -> toasty::ModelSet {
        let mut models = toasty::ModelSet::new();
        EventRecord::register(&mut models);
        SnapshotRecord::register(&mut models);
        models
    }

    /// Returns a handle sharing the pool, for explicit schema setup or queries.
    pub fn database(&self) -> toasty::Db {
        self.database.clone()
    }

    /// Snapshot cache sharing this store's database and connection pool.
    /// `connect` registers its model by default. Custom databases passed to
    /// `new` must register [`SnapshotRecord`] too. No schema changes are made.
    pub fn snapshot_cache(&self) -> SnapshotCache {
        SnapshotCache::new(self.database())
    }

    async fn append(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        events: &[StoredEvent],
        expected_version: u64,
    ) -> Result<(), AppendError> {
        let mut database = self.database();
        let mut tx = database.transaction().await?;
        let result = async {
            let actual_version = stream_version(&mut tx, aggregate_type, aggregate_id).await?;
            if actual_version != expected_version {
                return Err(AppendError::Store(CqrsError::Conflict {
                    expected_version,
                    actual_version,
                }));
            }
            for event in events {
                toasty::create!(EventRecord {
                    id: event.id.clone(),
                    aggregate_type: event.aggregate_type.clone(),
                    aggregate_id: event.aggregate_id.clone(),
                    version: event.version as i64,
                    event_type: event.event_type.clone(),
                    schema_version: i64::from(event.schema_version),
                    payload: serde_json::to_string(&event.payload)?,
                    metadata: serde_json::to_string(&event.metadata)?,
                    timestamp: event.timestamp.to_rfc3339(),
                })
                .exec(&mut tx)
                .await?;
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => tx.commit().await.map_err(AppendError::Database),
            Err(error) => {
                // Finish rollback before another attempt can acquire a connection.
                if let Err(rollback) = tx.rollback().await {
                    let original = match error {
                        AppendError::Store(error) => error,
                        AppendError::Database(error) => CqrsError::storage("append events", error),
                    };
                    let context =
                        format!("append failed: {original}; rollback also failed: {rollback}");
                    return Err(AppendError::Store(CqrsError::Other(
                        anyhow::Error::new(original).context(context),
                    )));
                }
                Err(error)
            }
        }
    }
}

impl EventStore for EventLog {
    async fn save_events(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        events: &[NewEvent],
        expected_version: u64,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        if events
            .iter()
            .any(|event| event.schema_version == 0 || event.event_type.is_empty())
        {
            return Err(CqrsError::EventFormat(
                "event kind and positive schema revision are required".into(),
            ));
        }
        let final_version = u64::try_from(events.len())
            .ok()
            .and_then(|count| expected_version.checked_add(count))
            .filter(|version| *version <= i64::MAX as u64)
            .ok_or_else(|| CqrsError::EventStore("stream version exceeds i64::MAX".into()))?;
        let persisted: Vec<_> = events
            .iter()
            .zip((expected_version..final_version).map(|version| version + 1))
            .map(|(event, version)| StoredEvent {
                id: uuid::Uuid::new_v4().to_string(),
                aggregate_type: aggregate_type.to_owned(),
                aggregate_id: aggregate_id.to_owned(),
                version,
                event_type: event.event_type.clone(),
                schema_version: event.schema_version,
                payload: event.payload.clone(),
                metadata: event.metadata.clone(),
                timestamp: event.timestamp,
                global_sequence: None,
            })
            .collect();

        for attempt in 0..=8 {
            match self
                .append(aggregate_type, aggregate_id, &persisted, expected_version)
                .await
            {
                Ok(()) => return Ok(persisted),
                Err(AppendError::Store(error)) => return Err(error),
                Err(AppendError::Database(error))
                    if error.is_serialization_failure() && attempt < 8 =>
                {
                    tokio::time::sleep(Duration::from_millis(10 << attempt.min(5))).await;
                }
                Err(AppendError::Database(error)) => {
                    return Err(CqrsError::storage("event database operation", error));
                }
            }
        }
        unreachable!("each final attempt returns")
    }

    async fn stream_version(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<u64, CqrsError> {
        let mut database = self.database();
        stream_version(&mut database, aggregate_type, aggregate_id)
            .await
            .map_err(|error| match error {
                AppendError::Store(error) => error,
                AppendError::Database(error) => {
                    CqrsError::storage("event database operation", error)
                }
            })
    }

    async fn list_streams_after(
        &self,
        after: Option<&StreamKey>,
        limit: NonZeroUsize,
    ) -> Result<Vec<StreamKey>, CqrsError> {
        i64::try_from(limit.get())
            .map_err(|_| CqrsError::EventStore("page size exceeds signed storage range".into()))?;
        let mut database = self.database();
        let mut cursor = after.cloned();
        let mut streams = Vec::new();
        // Toasty 0.11 has no public DISTINCT builder. Seek one identity at a
        // time, projecting only key columns and skipping its entire history.
        while streams.len() < limit.get() {
            let fields = EventRecord::fields();
            let mut query = EventRecord::all();
            if let Some((kind, id)) = &cursor {
                query = query.filter(
                    fields.aggregate_type().gt(kind.clone()).or(fields
                        .aggregate_type()
                        .eq(kind.clone())
                        .and(fields.aggregate_id().gt(id.clone()))),
                );
            }
            let keys = query
                .order_by((fields.aggregate_type().asc(), fields.aggregate_id().asc()))
                .limit(1)
                .select((fields.aggregate_type(), fields.aggregate_id()))
                .exec(&mut database)
                .await
                .map_err(|error| CqrsError::storage("event database operation", error))?;
            let Some(key) = keys.into_iter().next() else {
                break;
            };
            cursor = Some(key.clone());
            streams.push(key);
        }
        Ok(streams)
    }

    async fn load_events_after(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        after_version: u64,
        through_version: u64,
        limit: NonZeroUsize,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        if after_version > through_version {
            return Err(CqrsError::EventStore(
                "event range starts after its endpoint".into(),
            ));
        }
        let after = i64::try_from(after_version)
            .map_err(|_| CqrsError::EventStore("event version exceeds i64::MAX".into()))?;
        let through = i64::try_from(through_version)
            .map_err(|_| CqrsError::EventStore("event version exceeds i64::MAX".into()))?;
        i64::try_from(limit.get())
            .map_err(|_| CqrsError::EventStore("page size exceeds signed storage range".into()))?;
        if after == through {
            return Ok(Vec::new());
        }
        let mut database = self.database();
        let fields = EventRecord::fields();
        let records =
            EventRecord::filter_by_aggregate_type_and_aggregate_id(aggregate_type, aggregate_id)
                .filter(fields.version().gt(after).and(fields.version().le(through)))
                .order_by(fields.version().asc())
                .limit(limit.get())
                .exec(&mut database)
                .await
                .map_err(|error| CqrsError::storage("event database operation", error))?;
        records
            .into_iter()
            .map(EventRecord::into_event)
            .collect::<Result<Vec<_>, _>>()
    }
}

async fn stream_version(
    executor: &mut dyn Executor,
    aggregate_type: &str,
    aggregate_id: &str,
) -> Result<u64, AppendError> {
    let records =
        EventRecord::filter_by_aggregate_type_and_aggregate_id(aggregate_type, aggregate_id)
            .order_by(EventRecord::fields().version().desc())
            .limit(1)
            .select(EventRecord::fields().version())
            .exec(executor)
            .await?;
    records
        .first()
        .map_or(Ok(0), |version| positive_version(*version))
        .map_err(AppendError::Store)
}

fn positive_version(version: i64) -> Result<u64, CqrsError> {
    u64::try_from(version)
        .ok()
        .filter(|version| *version > 0)
        .ok_or_else(|| CqrsError::EventStore(format!("invalid persisted version: {version}")))
}

enum AppendError {
    Store(CqrsError),
    Database(toasty::Error),
}

impl From<toasty::Error> for AppendError {
    fn from(error: toasty::Error) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for AppendError {
    fn from(error: serde_json::Error) -> Self {
        Self::Store(error.into())
    }
}
