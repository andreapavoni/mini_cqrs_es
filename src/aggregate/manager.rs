use serde::{Serialize, de::DeserializeOwned};
use std::future::Future;

use crate::{
    Aggregate, AggregateSnapshot, CqrsError, DEFAULT_PAGE_SIZE, EventStore, EventStreamReader,
    LoadedAggregate, SnapshotStore,
};

/// The `AggregateManager` trait defines the behavior for loading and storing the state of aggregates.
///
/// Each implementation declares the aggregate it supports. Event-only managers
/// need no state serialization; snapshot managers can add Serde bounds on `A`.
/// All methods take `&self` to allow concurrent access.
pub trait AggregateManager<A: Aggregate>: Send + Sync {
    /// Loads an aggregate by its ID.
    fn load(
        &self,
        aggregate_id: &A::Id,
    ) -> impl Future<Output = Result<LoadedAggregate<A>, CqrsError>> + Send;

    /// Stores an aggregate's state. Default implementation is a no-op.
    fn store(
        &self,
        _aggregate: &LoadedAggregate<A>,
    ) -> impl Future<Output = Result<(), CqrsError>> + Send {
        async { Ok(()) }
    }
}

/// A simple implementation of the `AggregateManager` trait. It loads aggregates
/// by replaying their events from the associated `EventStore`, but doesn't implement any storage logic.
/// A successful empty history initializes a new aggregate; load failures propagate
/// to the caller rather than being treated as an absent aggregate.
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

impl<A, ES> AggregateManager<A> for SimpleAggregateManager<ES>
where
    A: Aggregate,
    ES: EventStore,
{
    async fn load(&self, aggregate_id: &A::Id) -> Result<LoadedAggregate<A>, CqrsError> {
        recover(
            &self.event_store,
            LoadedAggregate::new(aggregate_id.clone(), 0, A::default()),
        )
        .await
    }
}

// Both managers validate and apply the same bounded event pages. The snapshot
// manager supplies a recovered prefix; the simple manager starts at version zero.
async fn recover<A: Aggregate, ES: EventStore>(
    event_store: &ES,
    mut aggregate: LoadedAggregate<A>,
) -> Result<LoadedAggregate<A>, CqrsError> {
    let id = aggregate.id().to_string();
    let mut reader = EventStreamReader::open(
        event_store,
        A::aggregate_type(),
        &id,
        aggregate.version(),
        DEFAULT_PAGE_SIZE,
    )
    .await?;
    while let Some(events) = reader.next_page().await? {
        aggregate.apply_events(&events).await?;
    }
    Ok(aggregate)
}

/// Recovers state from an optional snapshot followed by its committed event tail.
///
/// Snapshots optimize recovery; events remain the source of truth. An absent
/// snapshot replays from version zero. Storage/decoding errors, wrong identity,
/// snapshots ahead of history, and invalid event tails propagate to the caller.
/// The snapshot is read before capturing a fixed event tip; later appends are
/// excluded. Only this manager requires Serde on domain state.
pub struct SnapshotAggregateManager<ES, SS>
where
    ES: EventStore,
    SS: SnapshotStore,
{
    event_store: ES,
    snapshot_store: SS,
}

impl<ES, SS> SnapshotAggregateManager<ES, SS>
where
    ES: EventStore,
    SS: SnapshotStore,
{
    /// Uses the same event history as the command runner, plus an optional-state
    /// cache. Snapshot storage is independent of the event append transaction.
    pub fn new(event_store: ES, snapshot_store: SS) -> Self {
        Self {
            event_store,
            snapshot_store,
        }
    }
}

impl<A, ES, SS> AggregateManager<A> for SnapshotAggregateManager<ES, SS>
where
    A: Aggregate + Serialize + DeserializeOwned,
    ES: EventStore,
    SS: SnapshotStore,
{
    async fn load(&self, aggregate_id: &A::Id) -> Result<LoadedAggregate<A>, CqrsError> {
        let aggregate = match self.snapshot_store.load_snapshot::<A>(aggregate_id).await? {
            Some(snapshot) => {
                if &snapshot.aggregate_id != aggregate_id {
                    return Err(CqrsError::SnapshotStore(
                        "snapshot belongs to a different aggregate".into(),
                    ));
                }
                let state = snapshot.get_payload()?;
                LoadedAggregate::new(snapshot.aggregate_id, snapshot.version, state)
            }
            None => LoadedAggregate::new(aggregate_id.clone(), 0, A::default()),
        };
        recover(&self.event_store, aggregate).await
    }

    async fn store(&self, aggregate: &LoadedAggregate<A>) -> Result<(), CqrsError> {
        self.snapshot_store
            .save_snapshot::<A>(AggregateSnapshot::new(aggregate)?)
            .await
    }
}
