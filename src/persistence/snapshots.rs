//! Optional snapshot cache in the event/support database.

use toasty::schema::Model;

use crate::{Aggregate, AggregateSnapshot, CqrsError, SnapshotStore};

/// Infrastructure record for an aggregate's last saved snapshot.
///
/// Register in the event/support database, separate from disposable projections.
/// A single upsert replaces payload and version atomically. This is a cache, not
/// event history: an older accurate snapshot may replace a newer one safely.
#[derive(Debug, toasty::Model)]
#[table = "cqrs_snapshots"]
#[key(aggregate_type, aggregate_id)]
pub struct SnapshotRecord {
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub version: i64,
    pub payload: String,
}

/// A Toasty snapshot cache implementing [`SnapshotStore`].
///
/// Prefer [`super::EventLog::snapshot_cache`] to share its pool and
/// registered models. Schema setup is explicit; no constructor performs DDL.
/// Versions range from zero through `i64::MAX`, matching the event store.
/// Save errors may have an unknown outcome; state/version are never partially
/// replaced. Concurrent saves can retain an older snapshot, recovered via its
/// event tail. Snapshots do not participate in event-append transactions.
#[derive(Clone)]
pub struct SnapshotCache {
    database: toasty::Db,
}

impl SnapshotCache {
    /// Uses a database whose model set includes [`SnapshotRecord`].
    pub fn new(database: toasty::Db) -> Self {
        Self { database }
    }

    /// Connects with only the snapshot model registered; does not create tables.
    pub async fn connect(url: &str) -> toasty::Result<Self> {
        let database = toasty::Db::builder()
            .models(Self::models())
            .connect(url)
            .await?;
        Ok(Self::new(database))
    }

    /// Snapshot-only model set for a separately configured cache database.
    /// The default event-store model set already includes this model.
    pub fn models() -> toasty::ModelSet {
        let mut models = toasty::ModelSet::new();
        SnapshotRecord::register(&mut models);
        models
    }

    /// Handle sharing the pool, for explicit schema setup or cache maintenance.
    pub fn database(&self) -> toasty::Db {
        self.database.clone()
    }
}

impl SnapshotStore for SnapshotCache {
    async fn save_snapshot<A: Aggregate>(
        &self,
        snapshot: AggregateSnapshot<A>,
    ) -> Result<(), CqrsError> {
        let version = i64::try_from(snapshot.version)
            .map_err(|_| CqrsError::SnapshotStore("snapshot version exceeds i64::MAX".into()))?;
        let payload = serde_json::to_string(snapshot.payload())?;
        SnapshotRecord::upsert_by_aggregate_type_and_aggregate_id(
            A::aggregate_type(),
            snapshot.aggregate_id.to_string(),
        )
        .version(version)
        .payload(payload)
        .exec(&mut self.database())
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn load_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
    ) -> Result<Option<AggregateSnapshot<A>>, CqrsError> {
        let record = SnapshotRecord::filter_by_aggregate_type_and_aggregate_id(
            A::aggregate_type(),
            aggregate_id.to_string(),
        )
        .first()
        .exec(&mut self.database())
        .await
        .map_err(storage_error)?;
        record
            .map(|record| {
                let version = u64::try_from(record.version).map_err(|_| {
                    CqrsError::SnapshotStore("snapshot version must not be negative".into())
                })?;
                Ok(AggregateSnapshot::from_parts(
                    aggregate_id.clone(),
                    version,
                    serde_json::from_str(&record.payload)?,
                ))
            })
            .transpose()
    }
}

fn storage_error(error: toasty::Error) -> CqrsError {
    CqrsError::storage("snapshot database operation", error)
}
