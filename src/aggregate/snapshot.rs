use std::future::Future;

use crate::{Aggregate, CqrsError};

/// The `SnapshotStore` trait defines the behavior for storing and loading aggregate snapshots.
///
/// All methods take `&self` to allow concurrent access.
pub trait SnapshotStore: Send + Sync {
    /// Saves an aggregate snapshot to the snapshot store.
    fn save_snapshot<T>(
        &self,
        aggregate: AggregateSnapshot<T>,
    ) -> impl Future<Output = Result<(), CqrsError>> + Send
    where
        T: Aggregate;

    /// Loads an aggregate snapshot from the snapshot store.
    fn load_snapshot<T>(
        &self,
        aggregate_id: &T::Id,
    ) -> impl Future<Output = Result<AggregateSnapshot<T>, CqrsError>> + Send
    where
        T: Aggregate;
}

/// The `AggregateSnapshot` struct represents a snapshot of an aggregate's state at a specific version.
#[derive(Clone, Debug)]
pub struct AggregateSnapshot<T>
where
    T: Aggregate,
{
    /// The ID of the aggregate.
    pub aggregate_id: T::Id,

    /// The serialized payload of the aggregate.
    payload: serde_json::Value,

    /// The version of the aggregate snapshot.
    pub version: u64,

    /// A marker to ensure type safety.
    marker: std::marker::PhantomData<T>,
}

impl<T> AggregateSnapshot<T>
where
    T: Aggregate,
{
    /// Creates a new aggregate snapshot.
    pub fn new(aggregate: &T, version: Option<u64>) -> Result<Self, CqrsError> {
        let version = version.unwrap_or(1);

        Ok(Self {
            aggregate_id: aggregate.aggregate_id(),
            payload: serde_json::to_value(aggregate)?,
            version,
            marker: std::marker::PhantomData,
        })
    }

    /// Gets the aggregate from the snapshot.
    pub fn get_payload<A>(&self) -> Result<A, CqrsError>
    where
        A: Aggregate,
    {
        Ok(serde_json::from_value(self.payload.clone())?)
    }
}
