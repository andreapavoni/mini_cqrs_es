use async_trait::async_trait;
use uuid::Uuid;

use crate::{Aggregate, CqrsError};

/// Snapshots are used to optimize loading aggregate state by allowing you to restore an aggregate
/// to a specific version without reapplying all the events from the beginning.
#[derive(Clone, Debug)]
pub struct AggregateSnapshot<T> where T: Aggregate {
    pub aggregate_id: Uuid,
    payload: serde_json::Value,
    pub version: u64,
    marker: std::marker::PhantomData<T>,
}

impl<T> AggregateSnapshot<T> where T: Aggregate {
    pub fn new(aggregate: &T, version: Option<u64>) -> Self {
        let version = version.unwrap_or(1);

        Self {
            aggregate_id: aggregate.aggregate_id(),
            payload: serde_json::to_value(aggregate).unwrap(),
            version,
            marker: std::marker::PhantomData,
        }
    }

    pub fn get_payload<A>(&self) -> A where A: Aggregate {
        serde_json::from_value(self.payload.clone()).unwrap()
    }
}

/// A trait representing a snapshot store for event-sourced aggregates.
///
/// Snapshot stores allow you to save and load snapshots of an aggregate's state.
#[async_trait]
pub trait SnapshotStore {
    /// Saves an aggregate snapshot to the snapshot store.
    async fn save_snapshot<T>(
        &mut self,
        aggregate: AggregateSnapshot<T>,
    ) -> Result<(), CqrsError> where T: Aggregate;

    /// Loads an aggregate snapshot from the snapshot store.
    async fn load_snapshot<T>(
        &self,
        aggregate_id: Uuid,
    ) -> Result<AggregateSnapshot<T>, CqrsError> where T: Aggregate;
}
