use async_trait::async_trait;

use crate::{Aggregate, CqrsError};

/// A snapshot of an event-sourced aggregate's state at a specific version.
///
/// Snapshots are used to optimize loading aggregate state by allowing you to restore an aggregate
/// to a specific version without reapplying all the events from the beginning.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::{Aggregate, AggregateSnapshot};
///
/// let aggregate = YourAggregate::default();
/// let snapshot = AggregateSnapshot::new(aggregate, Some(5));
/// ```
#[derive(Clone, Debug)]
pub struct AggregateSnapshot<T: Aggregate> {
    /// The identifier of the associated aggregate.
    pub aggregate_id: T::Id,

    /// The serialized payload representing the aggregate's state at the given version.
    payload: serde_json::Value,

    /// The version of the aggregate state at the time of the snapshot.
    pub version: u64,
}

impl<T: Aggregate> AggregateSnapshot<T> {
    /// Creates a new aggregate snapshot.
    ///
    /// # Parameters
    ///
    /// - `payload`: The aggregate whose state is being snapshot.
    /// - `version`: The version of the aggregate state at the time of the snapshot.
    ///
    /// # Example
    ///
    /// ```rust
    /// use mini_cqrs::{Aggregate, AggregateSnapshot};
    ///
    /// let aggregate = YourAggregate::default();
    /// let snapshot = AggregateSnapshot::new(aggregate, Some(5));
    /// ```
    pub fn new(payload: T, version: Option<u64>) -> Self {
        let version = version.unwrap_or(1);

        Self {
            aggregate_id: payload.aggregate_id(),
            payload: serde_json::to_value(payload).unwrap(),
            version,
        }
    }

    /// Retrieves the deserialized payload from the snapshot.
    ///
    /// # Returns
    ///
    /// The deserialized aggregate state at the time of the snapshot.
    ///
    /// # Example
    ///
    /// ```rust
    /// use mini_cqrs::{Aggregate, AggregateSnapshot};
    ///
    /// let aggregate = YourAggregate::default();
    /// let snapshot = AggregateSnapshot::new(aggregate.clone(), Some(5));
    ///
    /// let restored_aggregate = snapshot.get_payload();
    /// assert_eq!(restored_aggregate, aggregate);
    /// ```
    pub fn get_payload(&self) -> T {
        serde_json::from_value(self.payload.clone()).unwrap()
    }
}

/// A trait representing a snapshot store for event-sourced aggregates.
///
/// Snapshot stores allow you to save and load snapshots of an aggregate's state.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::{CqrsError, AggregateSnapshot, SnapshotStore};
///
/// struct YourAggregateSnapshotStore;
///
/// impl SnapshotStore<YourAggregate> for YourAggregateSnapshotStore {
///     // Implement snapshot store methods here.
/// }
/// ```
#[async_trait]
pub trait SnapshotStore<T: Aggregate> {
    /// Saves an aggregate snapshot to the snapshot store.
    ///
    /// # Parameters
    ///
    /// - `aggregate`: The snapshot of the aggregate's state.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an error if saving the snapshot fails.
    async fn save_snapshot(
        &mut self,
        aggregate: AggregateSnapshot<T>,
    ) -> Result<(), CqrsError>;

    /// Loads an aggregate snapshot from the snapshot store.
    ///
    /// # Parameters
    ///
    /// - `aggregate_id`: The identifier of the aggregate for which to load the snapshot.
    ///
    /// # Returns
    ///
    /// A `Result` containing the loaded snapshot or an error if loading fails.
    async fn load_snapshot(
        &self,
        aggregate_id: <T as Aggregate>::Id,
    ) -> Result<AggregateSnapshot<T>, CqrsError>;
}
