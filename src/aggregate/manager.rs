use crate::{Aggregate, AggregateSnapshot, EventStore, SnapshotStore, Uuid};
use anyhow::Error;
use async_trait::async_trait;

/// The `AggregateManager` trait defines the behavior for loading and storing the state of aggregates.
///
/// To create your custom aggregate manager, you need to implement this trait. The two main methods to implement
/// are `load` and `store`, allowing you to specify how aggregates are loaded and stored.
///
#[async_trait]
pub trait AggregateManager: Clone + Send + Sync {
    /// Loads an aggregate from the event store.
    async fn load<A>(&mut self, aggregate_id: Uuid) -> Result<A, Error>
    where
        A: Aggregate + Clone;

    /// Stores an aggregate to the event store.
    async fn store<A>(&mut self, _aggregate: &A) -> Result<(), Error>
    where
        A: Aggregate + Clone,
    {
        Ok(())
    }
}

/// A simple implementation of the `AggregateManager` trait. It loads aggregates
/// by replaying their events from the associated `EventStore`, but doesn't implement any storage logic.
///
#[derive(Clone)]
pub struct SimpleAggregateManager<ES>
where
    ES: EventStore + Send + Sync,
{
    event_store: ES,
}

impl<ES> SimpleAggregateManager<ES>
where
    ES: EventStore + Send + Sync,
{
    pub fn new(event_store: ES) -> Self {
        Self { event_store }
    }
}

#[async_trait]
impl<ES> AggregateManager for SimpleAggregateManager<ES>
where
    // Add necessary bounds: ES must be Clone to be used in Cqrs::new if manager is cloned,
    // Send + Sync likely needed because load is async.
    ES: EventStore + Clone + Send + Sync + 'static, // Add 'static if needed by async trait bounds
{
    async fn load<A>(&mut self, aggregate_id: Uuid) -> Result<A, Error>
    where
        A: Aggregate + Clone, // Add Send + Sync to A if needed by apply_events
    {
        let events = self.event_store.load_events(aggregate_id).await?;

        let mut aggregate = A::default();
        aggregate.set_aggregate_id(aggregate_id); // Set ID before applying events

        aggregate.apply_events(&events).await; // Apply loaded events

        Ok(aggregate)
    }
}

/// An aggregate manager that uses a snapshot store to load and store aggregates.
///
/// This implementation of the `AggregateManager` trait optimizes the loading of aggregates by utilizing a
/// `SnapshotStore`. Snapshots capture the aggregate state at specific points, reducing the need to replay
/// all events from the beginning.
#[derive(Clone)]
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

#[async_trait]
impl<SS> AggregateManager for SnapshotAggregateManager<SS>
where
    SS: SnapshotStore + Clone + Send + Sync,
{
    async fn load<A>(&mut self, aggregate_id: Uuid) -> Result<A, Error>
    where
        A: Aggregate + Clone,
    {
        if let Ok(snapshot) = self.snapshot_store.load_snapshot::<A>(aggregate_id).await {
            Ok(snapshot.get_payload())
        } else {
            let mut aggregate = A::default();
            aggregate.set_aggregate_id(aggregate_id);
            Ok(aggregate)
        }
    }

    async fn store<A>(&mut self, aggregate: &A) -> Result<(), Error>
    where
        A: Aggregate + Clone,
    {
        self.snapshot_store
            .save_snapshot::<A>(AggregateSnapshot::new(aggregate, None))
            .await?;
        Ok(())
    }
}
