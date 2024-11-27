use async_trait::async_trait;

use crate::{Aggregate, Uuid};
use anyhow::Error;

/// The `SnapshotStore` trait defines the behavior for storing and loading aggregate snapshots.
///
/// Aggregate snapshots are a copy of an aggregate's state at a specific point in time, allowing for optimized
/// loading of aggregates by reducing the need to replay all events from the beginning.
///
/// To create your custom snapshot store, you need to implement this trait. The two main methods to implement are
/// `save_snapshot` and `load_snapshot`, enabling you to define how snapshots are stored and loaded.
#[async_trait]
pub trait SnapshotStore {
    /// Saves an aggregate snapshot to the snapshot store.
    async fn save_snapshot<T>(&mut self, aggregate: AggregateSnapshot<T>) -> Result<(), Error>
    where
        T: Aggregate;

    /// Loads an aggregate snapshot from the snapshot store.
    async fn load_snapshot<T>(&self, aggregate_id: Uuid) -> Result<AggregateSnapshot<T>, Error>
    where
        T: Aggregate;
}

/// The `AggregateSnapshot` struct represents a snapshot of their wrapped aggregate.
///
/// An aggregate snapshot is created by serializing the state of an aggregate at a specific version. This structure
/// contains information about the aggregate ID, the serialized payload, the version, and a marker for type safety.
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
