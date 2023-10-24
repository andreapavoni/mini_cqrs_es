use std::fmt::Debug;

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

use crate::{CqrsError, EventPayload, Event};

pub mod snapshot;
pub mod manager;

/// A trait representing an event sourcing aggregate.
///
/// Aggregates are fundamental building blocks in event sourcing systems. They encapsulate domain
/// logic and state changes based on commands, emitting events as a result.
///
/// An aggregate:
/// - Receives commands through its `handle` method.
/// - Emits events in response to commands.
/// - Applies events to update its internal state.
/// - Tracks its unique identifier (`Id`) to associate events with the correct instance.
///
/// The `Aggregate` trait is designed to be generic and can be implemented for various aggregate
/// types. It includes methods for handling commands, applying events, and managing the aggregate's
/// unique identifier.
///
/// # Example
///
/// ```rust
/// use async_trait::async_trait;
/// use serde::{Serialize, Deserialize};
/// use crate::{CqrsError, Event, EventPayload};
///
/// #[derive(Debug, Serialize, Deserialize)]
/// struct MyEvent;
///
/// #[derive(Debug, Clone)]
/// struct MyCommand;
///
/// struct MyAggregate {
///     id: String,
/// }
///
/// #[async_trait]
/// impl Aggregate for MyAggregate {
///     type Command = MyCommand;
///     type Event = MyEvent;
///     type Id = String;
///
///     async fn handle(&self, command: Self::Command) -> Result<Vec<Event>, CqrsError> {
///         // Handle the command and generate events.
///         unimplemented!()
///     }
///
///     fn apply(&mut self, event: &Self::Event) {
///         // Apply the event to update the aggregate's state.
///         unimplemented!()
///     }
///
///     fn aggregate_id(&self) -> Uuid {
///         self.id.clone()
///     }
///
///     fn set_aggregate_id(&mut self, id: Uuid) {
///         self.id = id;
///     }
/// }
/// ```
#[async_trait]
pub trait Aggregate: Clone + Debug + Default + Sync + Send + Serialize + DeserializeOwned {
    type Command;
    type Event: EventPayload;

    async fn handle(&self, command: Self::Command) -> Result<Vec<Event>, CqrsError>;

    fn apply(&mut self, event: &Self::Event);
    fn aggregate_id(&self) -> Uuid;
    fn set_aggregate_id(&mut self, id: Uuid);

    fn apply_events(&mut self, events: &[Event]) {
        for e in events.iter() {
            self.apply(&e.get_payload::<Self::Event>());
        }
    }
}
