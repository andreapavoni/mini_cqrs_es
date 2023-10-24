use async_trait::async_trait;
use uuid::Uuid;

use crate::{Aggregate, CqrsError};

/// A trait that defines the behavior of a snapshot store.
///
/// A snapshot store is responsible for storing and loading aggregate snapshots.
///
/// This trait must be implemented by all snapshot stores in your application.
#[async_trait]
pub trait SnapshotStore {
    /// Saves an aggregate snapshot to the snapshot store.
    async fn save_snapshot<T>(&mut self, aggregate: AggregateSnapshot<T>) -> Result<(), CqrsError>
    where
        T: Aggregate;

    /// Loads an aggregate snapshot from the snapshot store.
    async fn load_snapshot<T>(&self, aggregate_id: Uuid) -> Result<AggregateSnapshot<T>, CqrsError>
    where
        T: Aggregate;
}

/// A struct that represents an aggregate snapshot.
///
/// An aggregate snapshot is a copy of the state of an aggregate at a given point in time.
#[derive(Clone, Debug)]
pub struct AggregateSnapshot<T>
where
    T: Aggregate,
{
    /// The ID of the aggregate.
    pub aggregate_id: Uuid,

    /// The serialized payload of the aggregate.
    payload: serde_json::Value,

    /// The version of the aggregate snapshot.
    pub version: u64,

    /// A marker to ensure that only aggregates of the correct type can be deserialized from the payload.
    marker: std::marker::PhantomData<T>,
}

impl<T> AggregateSnapshot<T>
where
    T: Aggregate,
{
    /// Creates a new aggregate snapshot.
    pub fn new(aggregate: &T, version: Option<u64>) -> Self {
        let version = version.unwrap_or(1);

        Self {
            aggregate_id: aggregate.aggregate_id(),
            payload: serde_json::to_value(aggregate).unwrap(),
            version,
            marker: std::marker::PhantomData,
        }
    }

    /// Gets the aggregate from the snapshot.
    pub fn get_payload<A>(&self) -> A
    where
        A: Aggregate,
    {
        serde_json::from_value(self.payload.clone()).unwrap()
    }
}
