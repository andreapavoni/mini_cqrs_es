use crate::{Aggregate, AggregateSnapshot, CqrsError, EventStore, SnapshotStore};
use async_trait::async_trait;
use uuid::Uuid;

/// A trait that defines the behavior of an aggregate manager.
///
/// An aggregate manager is responsible for loading and storing aggregates.
///
/// This trait must be implemented by all aggregate managers in your application.
#[async_trait]
pub trait AggregateManager: Clone + Send + Sync {
    /// Loads an aggregate from the event store.
    async fn load<A>(&mut self, aggregate_id: Uuid) -> Result<A, CqrsError>
    where
        A: Aggregate + Clone;

    /// Stores an aggregate to the event store.
    async fn store<A>(&mut self, _aggregate: &A) -> Result<(), CqrsError>
    where
        A: Aggregate + Clone,
    {
        Ok(())
    }
}

/// A simple aggregate manager that loads aggregates by replaying all of their events.
#[derive(Clone)]
pub struct SimpleAggregateManager<'a, ES>
where
    ES: EventStore,
{
    event_store: &'a ES,
}

impl<'a, ES> SimpleAggregateManager<'a, ES>
where
    ES: EventStore,
{
    pub fn new(event_store: &'a ES) -> Self {
        Self { event_store }
    }
}

#[async_trait]
impl<'a, ES> AggregateManager for SimpleAggregateManager<'a, ES>
where
    ES: EventStore + Clone,
{
    async fn load<A: Aggregate>(&mut self, aggregate_id: Uuid) -> Result<A, CqrsError> {
        let mut aggregate = A::default();
        aggregate.set_aggregate_id(aggregate_id);

        if let Ok(events) = self.event_store.load_events(aggregate_id.clone()).await {
            aggregate.apply_events(&events).await;
            return Ok(aggregate);
        }

        Ok(aggregate)
    }
}

/// An aggregate manager that uses a snapshot store to load and store aggregates.
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
    async fn load<A>(&mut self, aggregate_id: Uuid) -> Result<A, CqrsError>
    where
        A: Aggregate + Clone,
    {
        if let Ok(snapshot) = self
            .snapshot_store
            .load_snapshot::<A>(aggregate_id.clone())
            .await
        {
            Ok(snapshot.get_payload())
        } else {
            let mut aggregate = A::default();
            aggregate.set_aggregate_id(aggregate_id);
            Ok(aggregate)
        }
    }

    async fn store<A>(&mut self, aggregate: &A) -> Result<(), CqrsError>
    where
        A: Aggregate + Clone,
    {
        self.snapshot_store
            .save_snapshot::<A>(AggregateSnapshot::new(aggregate, None))
            .await?;
        Ok(())
    }
}
