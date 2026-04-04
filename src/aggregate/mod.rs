use std::fmt::Debug;
use std::future::Future;

use serde::{de::DeserializeOwned, Serialize};

use crate::{CqrsError, Event, EventPayload, Uuid};

pub mod manager;
pub mod snapshot;

/// The `Aggregate` trait defines the behavior of an aggregate, which represents the state of a
/// domain entity and can be modified by applying events.
///
/// Aggregates track their version for optimistic concurrency control.
pub trait Aggregate: Clone + Debug + Default + Sync + Send + Serialize + DeserializeOwned {
    /// The type of event that this aggregate can handle.
    type Event: EventPayload + Send + Sync;

    /// Applies an event to the aggregate's state.
    fn apply(&mut self, event: &Self::Event) -> impl Future<Output = ()> + Send;

    /// Returns the aggregate's ID.
    fn aggregate_id(&self) -> Uuid;

    /// Sets the aggregate's ID.
    fn set_aggregate_id(&mut self, id: Uuid);

    /// Returns the current version of the aggregate.
    fn version(&self) -> u64 {
        0
    }

    /// Sets the version of the aggregate.
    fn set_version(&mut self, _version: u64) {}

    /// Applies a sequence of events to the aggregate's state.
    fn apply_events(
        &mut self,
        events: &[Event],
    ) -> impl Future<Output = Result<(), CqrsError>> + Send {
        async {
            for e in events.iter() {
                self.apply(&e.get_payload::<Self::Event>()?).await;
            }
            Ok(())
        }
    }
}
