use std::fmt::Debug;

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use crate::{CqrsError, EventPayload, Event};

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
///     fn aggregate_id(&self) -> Self::Id {
///         self.id.clone()
///     }
///
///     fn set_aggregate_id(&mut self, id: Self::Id) {
///         self.id = id;
///     }
/// }
/// ```
#[async_trait]
pub trait Aggregate: Debug + Default + Sync + Send + Serialize + DeserializeOwned {
    /// The type representing commands that can be handled by this aggregate.
    type Command;

    /// The type representing events emitted by this aggregate.
    type Event: EventPayload;

    /// The type representing the unique identifier of this aggregate.
    type Id: Clone + Send + Sync + Debug + ToString;

    /// Handles a command and returns a list of events generated as a result.
    ///
    /// This method receives a command, processes it, and returns a list of events that are emitted
    /// as a result of handling the command.
    ///
    /// # Parameters
    ///
    /// - `command`: The command to be handled.
    ///
    /// # Returns
    ///
    /// A `Result` containing a list of events or an error if command handling fails.
    async fn handle(&self, command: Self::Command) -> Result<Vec<Event>, CqrsError>;

    /// Applies an event to update the internal state of the aggregate.
    ///
    /// This method is responsible for applying an event to update the internal state of the
    /// aggregate. It mutates the aggregate based on the information in the event.
    ///
    /// # Parameters
    ///
    /// - `event`: The event to be applied.
    fn apply(&mut self, event: &Self::Event);

    /// Gets the unique identifier of the aggregate.
    ///
    /// This method returns the unique identifier associated with the aggregate instance. It is
    /// used to determine which aggregate instance should receive the events.
    ///
    /// # Returns
    ///
    /// The unique identifier of the aggregate.
    fn aggregate_id(&self) -> Self::Id;

    /// Sets the unique identifier of the aggregate.
    ///
    /// This method is used to set or update the unique identifier associated with the aggregate
    /// instance. It is typically called during the construction of the aggregate.
    ///
    /// # Parameters
    ///
    /// - `id`: The unique identifier to be set.
    fn set_aggregate_id(&mut self, id: Self::Id);

    /// Applies a list of events to update the internal state of the aggregate.
    ///
    /// This method applies a list of events to the aggregate in sequence, updating its internal
    /// state based on the information in each event.
    ///
    /// # Parameters
    ///
    /// - `events`: A slice of events to be applied.
    fn apply_events(&mut self, events: &[Event]) {
        for e in events.iter() {
            self.apply(&e.get_payload::<Self::Event>());
        }
    }
}
