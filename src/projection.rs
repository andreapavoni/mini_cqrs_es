//! Transactional Toasty read models with per-projection, per-stream checkpoints.

use std::future::Future;

use crate::{Aggregate, CqrsError, EventConsumer, StoredEvent};

/// Application read-model mutations for one aggregate's events.
///
/// Use only the supplied executor for persistence. Do not commit, roll back, run
/// transaction-control SQL, or perform external side effects in this callback.
pub trait Projection: Send + Sync {
    type Aggregate: Aggregate;

    /// Stable checkpoint namespace. Changed definitions require rebuilt outputs;
    /// changing this name alone does not reset already populated model tables.
    fn name() -> &'static str;

    fn apply(
        &self,
        event: &StoredEvent,
        payload: <Self::Aggregate as Aggregate>::Event,
        executor: &mut dyn toasty::Executor,
    ) -> impl Future<Output = Result<(), CqrsError>> + Send;
}

/// Rebuildable read-side state. Register this model alongside application models
/// in the read database's `ModelSet`, never in the event database's schema.
#[derive(Debug, toasty::Model)]
#[table = "projection_checkpoints"]
#[key(projection_name, aggregate_type, aggregate_id)]
pub struct ProjectionCheckpoint {
    pub projection_name: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub last_version: i64,
}

/// An event consumer committing a projection and its checkpoint atomically.
///
/// Requires a single ordered writer per projection/stream. Immutable historical
/// versions at or below the checkpoint are skipped before decoding; gaps fail.
/// The callback is never retried automatically. Only work through its executor
/// participates in the transaction. This does not coordinate the event database.
///
/// Commit errors and cancellation can leave an unknown outcome. Toasty queues
/// rollback when an unfinished transaction is dropped; cleanup is asynchronous.
/// Model changes and checkpoint remain atomic, even when no result is returned.
/// Clones share the read pool and projection definition; `P` need not be Clone.
pub struct Projector<P> {
    database: toasty::Db,
    projection: std::sync::Arc<P>,
}

impl<P> Clone for Projector<P> {
    fn clone(&self) -> Self {
        Self {
            database: self.database.clone(),
            projection: self.projection.clone(),
        }
    }
}

impl<P: Projection> Projector<P> {
    pub fn new(database: toasty::Db, projection: P) -> Self {
        Self {
            database,
            projection: std::sync::Arc::new(projection),
        }
    }

    /// Last committed version, or zero for an unprojected stream. This is a
    /// delivery checkpoint suitable for `replay_from`, unlike a reader position.
    pub async fn last_applied(
        &self,
        aggregate_id: &<P::Aggregate as Aggregate>::Id,
    ) -> Result<u64, CqrsError> {
        let mut database = self.database.clone();
        let checkpoint = self
            .lookup(&mut database, &aggregate_id.to_string())
            .await?;
        Ok(current_version(checkpoint.as_ref())? as u64)
    }

    /// Resume this projection from its committed checkpoint through one captured
    /// tip. Coordinate live delivery so there is one ordered writer per stream.
    /// On failure earlier events remain committed and a later call resumes them.
    pub async fn catch_up(
        &self,
        events: &impl crate::EventStore,
        id: &<P::Aggregate as Aggregate>::Id,
    ) -> Result<u64, CqrsError> {
        let after = self.last_applied(id).await?;
        let encoded = id.to_string();
        let mut reader = crate::EventStreamReader::open(
            events,
            P::Aggregate::aggregate_type(),
            &encoded,
            after,
            crate::DEFAULT_PAGE_SIZE,
        )
        .await?;
        while let Some(page) = reader.next_page().await? {
            for event in &page {
                self.process(event).await?;
            }
        }
        Ok(reader.through_version())
    }

    async fn lookup(
        &self,
        executor: &mut dyn toasty::Executor,
        aggregate_id: &str,
    ) -> Result<Option<ProjectionCheckpoint>, CqrsError> {
        ProjectionCheckpoint::filter_by_projection_name_and_aggregate_type_and_aggregate_id(
            P::name(),
            P::Aggregate::aggregate_type(),
            aggregate_id,
        )
        .first()
        .exec(executor)
        .await
        .map_err(storage_error)
    }

    async fn apply_next(
        &self,
        executor: &mut dyn toasty::Executor,
        event: &StoredEvent,
        next: i64,
    ) -> Result<(), CqrsError> {
        let checkpoint = self.lookup(executor, &event.aggregate_id).await?;
        let current = current_version(checkpoint.as_ref())?;
        if next <= current {
            return Ok(());
        }
        if current.checked_add(1) != Some(next) {
            return Err(CqrsError::EventFormat(format!(
                "projection gap: last applied {current}, incoming {next}"
            )));
        }
        let payload = event.get_payload::<<P::Aggregate as Aggregate>::Event>()?;
        self.projection.apply(event, payload, executor).await?;
        match checkpoint {
            Some(mut checkpoint) => {
                checkpoint
                    .update()
                    .last_version(next)
                    .exec(executor)
                    .await
                    .map_err(storage_error)?;
            }
            None => {
                toasty::create!(ProjectionCheckpoint {
                    projection_name: P::name().to_owned(),
                    aggregate_type: event.aggregate_type.clone(),
                    aggregate_id: event.aggregate_id.clone(),
                    last_version: next,
                })
                .exec(executor)
                .await
                .map_err(storage_error)?;
            }
        }
        Ok(())
    }
}

impl<P: Projection> EventConsumer for Projector<P> {
    async fn process(&self, event: &StoredEvent) -> Result<(), CqrsError> {
        if event.aggregate_type != P::Aggregate::aggregate_type() {
            return Ok(());
        }
        let next = i64::try_from(event.version).map_err(|_| {
            CqrsError::EventFormat("projection event version exceeds i64::MAX".into())
        })?;
        if next == 0 {
            return Err(CqrsError::EventFormat(
                "event versions must be positive".into(),
            ));
        }
        let mut database = self.database.clone();
        let mut tx = database.transaction().await.map_err(storage_error)?;
        match self.apply_next(&mut tx, event, next).await {
            Ok(()) => tx.commit().await.map_err(|error| {
                CqrsError::storage("projection commit failed; outcome may be unknown", error)
            }),
            Err(error) => match tx.rollback().await {
                Ok(()) => Err(error),
                Err(rollback) => {
                    let context =
                        format!("projection failed: {error}; rollback also failed: {rollback}");
                    Err(CqrsError::Other(anyhow::Error::new(error).context(context)))
                }
            },
        }
    }
}

fn current_version(checkpoint: Option<&ProjectionCheckpoint>) -> Result<i64, CqrsError> {
    let version = checkpoint.map_or(0, |checkpoint| checkpoint.last_version);
    if version < 0 {
        return Err(CqrsError::EventFormat("negative checkpoint version".into()));
    }
    Ok(version)
}

fn storage_error(error: toasty::Error) -> CqrsError {
    CqrsError::storage("projection database operation", error)
}
