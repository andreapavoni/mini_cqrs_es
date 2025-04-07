use anyhow::Error;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{CqrsError, Result, Uuid};

/// The `Event` struct represents a change to the state of an aggregate in a CQRS application.
///
/// An event contains information such as its unique ID, event type, aggregate ID, payload data, version, and timestamp. These details
/// help capture the changes made to an aggregate's state, allowing it to be reconstructed.
///
/// Events are typically created using the `Event::new` constructor, which generates a new event based on an event payload.
///
/// You can extract the payload from an event using the `get_payload` method, which deserializes the payload data into a specific
/// type that implements the `EventPayload` trait.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// The ID of the event.
    pub id: String,

    /// The type of event.
    pub event_type: String,

    /// The ID of the aggregate that the event is associated with.
    pub aggregate_id: Uuid,

    /// The payload of the event.
    payload: serde_json::Value,

    /// The version of the event.
    pub version: u32,

    /// The progression number of the event.
    pub sequence_number: Option<u64>,

    /// The timestamp of the event.
    pub timestamp: DateTime<Utc>,
}

impl Event {
    /// Creates a new event.
    pub fn new<T: EventPayload>(payload: T, version: Option<u32>) -> Self {
        let version = version.unwrap_or(1);
        let timestamp = Utc::now();

        Self {
            id: Uuid::new_v4().to_string(),
            event_type: payload.name(),
            aggregate_id: payload.aggregate_id(),
            payload: serde_json::to_value(payload).unwrap(),
            version,
            sequence_number: None,
            timestamp,
        }
    }

    /// Gets the payload of the event.
    // pub fn get_payload<T: EventPayload>(&self) -> Result<T, CqrsError> {
    //     serde_json::from_value(self.payload.clone()).map_err(CqrsError::PayloadDeserialization)
    // }
    pub fn get_payload<T: EventPayload + DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_value(self.payload.clone()).map_err(CqrsError::PayloadDeserialization)
    }
}

/// The `wrap_event!` macro provides a convenient way to wrap an event payload type in an event type.
///
/// This macro generates the necessary implementations for conversions between an event payload type and an event type, allowing
/// easy integration with the `Event` struct.
#[macro_export]
macro_rules! wrap_event {
    ($evt: ident) => {
        impl From<Event> for $evt {
            fn from(evt: Event) -> Self {
                evt.get_payload::<$evt>().unwrap()
            }
        }

        impl Into<Event> for $evt {
            fn into(self) -> Event {
                Event::new(self, None)
            }
        }
    };
}

/// The `EventPayload` trait defines the behavior of an event payload, representing the change the event made to the state of an aggregate.
///
/// To create an event payload in your application, you should implement this trait for each specific event payload type.
pub trait EventPayload<Evt = Self>: Serialize + DeserializeOwned + Clone + ToString {
    /// Gets the ID of the aggregate that the event payload is associated with.
    fn aggregate_id(&self) -> Uuid;

    /// Gets the name of the event payload.
    fn name(&self) -> String {
        self.to_string()
    }
}

/// The `EventStore` trait defines the behavior for storing and loading events,
/// allowing the application to keep a historical record of state changes.
///
/// To create an event store in your application, you should implement this trait. You need to specify how to save and load events
/// associated with specific aggregate IDs.
#[async_trait]
pub trait EventStore: Send + Sync {
    async fn save_events(&mut self, aggregate_id: Uuid, events: &[Event]) -> Result<(), Error>;

    async fn load_events(&self, aggregate_id: Uuid) -> Result<Vec<Event>, Error>;
}
