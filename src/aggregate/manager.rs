use std::future::Future;

use crate::{Aggregate, AggregateSnapshot, CqrsError, EventStore, SnapshotStore, Uuid};

/// The `AggregateManager` trait defines the behavior for loading and storing the state of aggregates.
///
/// All methods take `&self` to allow concurrent access.
pub trait AggregateManager: Send + Sync {
    /// Loads an aggregate by its ID.
    fn load<A>(&self, aggregate_id: Uuid) -> impl Future<Output = Result<A, CqrsError>> + Send
    where
        A: Aggregate;

    /// Stores an aggregate's state. Default implementation is a no-op.
    fn store<A>(&self, _aggregate: &A) -> impl Future<Output = Result<(), CqrsError>> + Send
    where
        A: Aggregate,
    {
        async { Ok(()) }
    }
}

/// A simple implementation of the `AggregateManager` trait. It loads aggregates
/// by replaying their events from the associated `EventStore`, but doesn't implement any storage logic.
pub struct SimpleAggregateManager<ES>
where
    ES: EventStore,
{
    event_store: ES,
}

impl<ES> SimpleAggregateManager<ES>
where
    ES: EventStore,
{
    pub fn new(event_store: ES) -> Self {
        Self { event_store }
    }
}

impl<ES> AggregateManager for SimpleAggregateManager<ES>
where
    ES: EventStore,
{
    async fn load<A: Aggregate>(&self, aggregate_id: Uuid) -> Result<A, CqrsError> {
        let mut aggregate = A::default();
        aggregate.set_aggregate_id(aggregate_id);

        if let Ok((events, version)) = self.event_store.load_events(aggregate_id).await {
            aggregate.apply_events(&events).await?;
            aggregate.set_version(version);
        }

        Ok(aggregate)
    }
}

/// An aggregate manager that uses a snapshot store to load and store aggregates.
///
/// This implementation optimizes the loading of aggregates by utilizing a `SnapshotStore`.
/// Snapshots capture the aggregate state at specific points, reducing the need to replay
/// all events from the beginning.
pub struct SnapshotAggregateManager<SS>
where
    SS: SnapshotStore,
{
    snapshot_store: SS,
}

impl<SS> SnapshotAggregateManager<SS>
where
    SS: SnapshotStore,
{
    pub fn new(snapshot_store: SS) -> Self {
        Self { snapshot_store }
    }
}

impl<SS> AggregateManager for SnapshotAggregateManager<SS>
where
    SS: SnapshotStore,
{
    async fn load<A>(&self, aggregate_id: Uuid) -> Result<A, CqrsError>
    where
        A: Aggregate,
    {
        if let Ok(snapshot) = self.snapshot_store.load_snapshot::<A>(aggregate_id).await {
            let mut aggregate = snapshot.get_payload::<A>()?;
            aggregate.set_version(snapshot.version);
            Ok(aggregate)
        } else {
            let mut aggregate = A::default();
            aggregate.set_aggregate_id(aggregate_id);
            Ok(aggregate)
        }
    }

    async fn store<A>(&self, aggregate: &A) -> Result<(), CqrsError>
    where
        A: Aggregate,
    {
        self.snapshot_store
            .save_snapshot::<A>(AggregateSnapshot::new(aggregate, Some(aggregate.version()))?)
            .await?;
        Ok(())
    }
}
