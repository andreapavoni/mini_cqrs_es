use std::fmt::Debug;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

use crate::{Event, EventPayload, Uuid};

pub mod manager;
pub mod snapshot;

/// The `Aggregate` trait defines the behavior of an aggregate, which represent the state of a domain entity and can be modified by applying events.
///
/// Aggregates are a critical part of the `mini_cqrs_es` framework and must be implemented for each entity you want
/// to work with. This trait provides the fundamental methods that an aggregate should support.
///
/// ## Implementing the `Aggregate` Trait
///
/// To implement the `Aggregate` trait for your domain entities, you need to define the associated type `Event`,
/// which represents the type of event that your aggregate can handle. The `Event` type should implement the
/// `EventPayload` trait from `mini_cqrs_es`.
#[async_trait]
pub trait Aggregate: Clone + Debug + Default + Sync + Send + Serialize + DeserializeOwned {
    /// The type of event that this aggregate can handle.
    type Event: EventPayload + Send + Sync;

    /// Applies an event to the aggregate's state.
    async fn apply(&mut self, event: &Self::Event);

    /// Returns the aggregate's ID.
    fn aggregate_id(&self) -> Uuid;

    /// Sets the aggregate's ID.
    fn set_aggregate_id(&mut self, id: Uuid);

    /// Applies a sequence of events to the aggregate's state.
    async fn apply_events(&mut self, events: &[Event]) {
        for e in events.iter() {
            self.apply(&e.get_payload::<Self::Event>()).await;
        }
    }
}
