use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Serialize};
use uuid::Uuid;
use async_trait::async_trait;

use crate::CqrsError;

/// Represents an event in the event sourcing system.
///
/// Events capture changes in the state of aggregates and provide a historical record of
/// state transitions. Each event has a unique identifier, an event type, an associated
/// aggregate identifier, payload data, a version, and a timestamp.
///
/// Events are used to reconstruct the state of aggregates by applying them in sequence.
///
/// # Example
///
/// ```rust
/// use chrono::{DateTime, Utc};
/// use serde_json::json;
/// use mini_cqrs::Event;
///
/// // Create a new event instance.
/// let event = Event::new(json!({"field": "value"}), None);
///
/// // Access event properties.
/// assert_eq!(event.event_type, "YourEventType");
/// ```
#[derive(Clone, Debug)]
pub struct Event {
    /// The unique identifier of the event.
    pub id: String,

    /// The type of the event.
    pub event_type: String,

    /// The identifier of the aggregate associated with the event.
    pub aggregate_id: String,

    /// The payload data of the event.
    pub payload: serde_json::Value,

    /// The version of the event.
    pub version: u64,

    /// The timestamp when the event was created.
    pub timestamp: DateTime<Utc>,
}

impl Event {
    /// Creates a new event instance.
    ///
    /// # Parameters
    ///
    /// - `payload`: The payload data of the event.
    /// - `version`: An optional version number for the event (default is 1).
    ///
    /// # Returns
    ///
    /// A new `Event` instance.
    pub fn new<T: EventPayload>(payload: T, version: Option<u64>) -> Self {
        let version = version.unwrap_or(1);
        let timestamp = Utc::now();

        Self {
            id: Uuid::new_v4().to_string(),
            event_type: payload.name(),
            aggregate_id: payload.aggregate_id(),
            payload: serde_json::to_value(payload).unwrap(),
            version,
            timestamp,
        }
    }

    /// Extracts the payload data from the event.
    ///
    /// # Returns
    ///
    /// The payload data of the event.
    pub fn get_payload<T: EventPayload>(&self) -> T {
        serde_json::from_value(self.payload.clone()).unwrap()
    }
}

/// A macro for wrapping an event struct, enabling conversion to and from `Event`.
///
/// This macro simplifies the implementation of conversions between your custom event struct
/// and the generic `Event` struct used in the event sourcing system.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::{Event, wrap_event};
///
/// #[derive(Debug)]
/// struct MyEvent;
///
/// wrap_event!(MyEvent);
/// ```
#[macro_export]
macro_rules! wrap_event {
    ($evt: ident) => {
        impl From<Event> for $evt {
            fn from(evt: Event) -> Self {
                evt.get_payload::<$evt>()
            }
        }

        impl Into<Event> for $evt {
            fn into(self) -> Event {
                Event::new(self, None)
            }
        }
    };
}

/// A trait for event payloads.
///
/// Event payloads are the data associated with events. Each event type should implement this trait
/// to specify how to extract aggregate identifiers and event names from the payload data.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::EventPayload;
/// use serde_json::json;
///
/// #[derive(Debug)]
/// struct MyEvent {
///     aggregate_id: String,
/// }
///
/// impl EventPayload for MyEvent {
///     fn aggregate_id(&self) -> String {
///         self.aggregate_id.clone()
///     }
/// }
/// ```
pub trait EventPayload<Evt = Self>: Serialize + DeserializeOwned + Clone + ToString {
    /// Gets the aggregate identifier associated with the event payload.
    fn aggregate_id(&self) -> String;

    /// Gets the name of the event based on the payload.
    fn name(&self) -> String {
        self.to_string()
    }
}

/// A trait representing an event store for persisting and retrieving events.
///
/// Event stores are responsible for storing and retrieving events associated with aggregates.
///
/// # Example
///
/// ```rust
/// use async_trait::async_trait;
/// use mini_cqrs::{CqrsError, Event};
///
/// #[derive(Clone)]
/// struct YourEventStore;
///
/// #[async_trait]
/// impl EventStore for YourEventStore {
///     type AggregateId = String;
///
///     async fn save_events(
///         &mut self,
///         aggregate_id: Self::AggregateId,
///         events: &[Event],
///     ) -> Result<(), CqrsError> {
///         // Implement event saving logic here.
///         unimplemented!()
///     }
///
///     async fn load_events(
///         &self,
///         aggregate_id: Self::AggregateId,
///     ) -> Result<Vec<Event>, CqrsError> {
///         // Implement event loading logic here.
///         unimplemented!()
///     }
/// }
/// ```
#[async_trait]
pub trait EventStore: Send + Sync {
    /// The type representing the unique identifier of an aggregate.
    type AggregateId: Clone;

    /// Saves a list of events associated with an aggregate.
    ///
    /// # Parameters
    ///
    /// - `aggregate_id`: The unique identifier of the aggregate.
    /// - `events`: A slice of events to be saved.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an error if saving events fails.
    async fn save_events(
        &mut self,
        aggregate_id: Self::AggregateId,
        events: &[Event],
    ) -> Result<(), CqrsError>;

    /// Loads events associated with an aggregate.
    ///
    /// # Parameters
    ///
    /// - `aggregate_id`: The unique identifier of the aggregate.
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of events or an error if loading events fails.
    async fn load_events(
        &self,
        aggregate_id: Self::AggregateId,
    ) -> Result<Vec<Event>, CqrsError>;
}
