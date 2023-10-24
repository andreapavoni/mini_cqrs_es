use std::fmt::Debug;

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use uuid::Uuid;

use crate::{Event, EventPayload};

pub mod manager;
pub mod snapshot;

/// A trait that defines the behavior of an aggregate.
///
/// Aggregates are the building blocks of CQRS applications. They represent the state of a domain entity
/// and can be mutated by applying events.
///
/// This trait must be implemented by all aggregates in your application.
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
