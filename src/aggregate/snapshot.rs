use serde::{Serialize, de::DeserializeOwned};
use std::future::Future;

use crate::{Aggregate, CqrsError, LoadedAggregate};

/// The `SnapshotStore` trait defines the behavior for storing and loading aggregate snapshots.
///
/// All methods take `&self` to allow concurrent access.
/// Stores must isolate snapshots by (`Aggregate::aggregate_type()`, aggregate ID)
/// and preserve state and version together. A snapshot must represent domain state
/// after exactly its version's committed events; the manager trusts that prefix
/// and validates only the subsequent event tail. Older snapshots remain valid.
/// Version zero is allowed and must represent the aggregate's default initial state.
/// The generic envelope does not contain a persisted type discriminator: stores
/// are responsible for enforcing the type namespace when reading and writing.
pub trait SnapshotStore: Send + Sync {
    /// Saves committed state and metadata atomically; a failure must not expose a
    /// partially written snapshot. This operation is separate from event append;
    /// a save failure does not undo events already committed.
    fn save_snapshot<T>(
        &self,
        aggregate: AggregateSnapshot<T>,
    ) -> impl Future<Output = Result<(), CqrsError>> + Send
    where
        T: Aggregate;

    /// Returns `None` only when no snapshot exists for this type/ID. Storage or
    /// decoding failures must be returned as errors, never disguised as absence.
    fn load_snapshot<T>(
        &self,
        aggregate_id: &T::Id,
    ) -> impl Future<Output = Result<Option<AggregateSnapshot<T>>, CqrsError>> + Send
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
    /// Restores an envelope from snapshot storage without decoding domain state.
    /// The store must preserve the original identity, version, and JSON payload.
    pub fn from_parts(aggregate_id: T::Id, version: u64, payload: serde_json::Value) -> Self {
        Self {
            aggregate_id,
            version,
            payload,
            marker: std::marker::PhantomData,
        }
    }

    /// Serialized domain state, for storage without requiring Serde on `T`.
    pub fn payload(&self) -> &serde_json::Value {
        &self.payload
    }

    /// Captures domain state and the runtime's exact identity/version.
    pub fn new(aggregate: &LoadedAggregate<T>) -> Result<Self, CqrsError>
    where
        T: Serialize,
    {
        Ok(Self {
            aggregate_id: aggregate.id().clone(),
            payload: serde_json::to_value(aggregate.state())?,
            version: aggregate.version(),
            marker: std::marker::PhantomData,
        })
    }

    /// Gets the aggregate from the snapshot.
    pub fn get_payload(&self) -> Result<T, CqrsError>
    where
        T: DeserializeOwned,
    {
        Ok(serde_json::from_value(self.payload.clone())?)
    }
}

impl<S: SnapshotStore + ?Sized> SnapshotStore for std::sync::Arc<S> {
    async fn save_snapshot<A: Aggregate>(
        &self,
        snapshot: AggregateSnapshot<A>,
    ) -> Result<(), CqrsError> {
        (**self).save_snapshot(snapshot).await
    }
    async fn load_snapshot<A: Aggregate>(
        &self,
        id: &A::Id,
    ) -> Result<Option<AggregateSnapshot<A>>, CqrsError> {
        (**self).load_snapshot(id).await
    }
}
