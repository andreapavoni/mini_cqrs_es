use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Serialize};
use uuid::Uuid;

use crate::CqrsError;

/// An event that represents a change to the state of an aggregate.
#[derive(Clone, Debug)]
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
    pub version: u64,

    /// The timestamp of the event.
    pub timestamp: DateTime<Utc>,
}

impl Event {
    /// Creates a new event.
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

    /// Gets the payload of the event.
    pub fn get_payload<T: EventPayload>(&self) -> T {
        serde_json::from_value(self.payload.clone()).unwrap()
    }
}

/// A macro that wraps an event payload type in an event type.
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

/// A trait that defines the behavior of an event payload.
///
/// An event payload is the data that is associated with an event. It is used to represent the change that the event made to the state of the aggregate.
///
/// This trait must be implemented by all event payloads in your application.
pub trait EventPayload<Evt = Self>: Serialize + DeserializeOwned + Clone + ToString {
    /// Gets the ID of the aggregate that the event payload is associated with.
    fn aggregate_id(&self) -> Uuid;

    /// Gets the name of the event payload.
    fn name(&self) -> String {
        self.to_string()
    }
}

/// A trait that defines the behavior of an event store.
///
/// An event store is responsible for storing and loading events.
///
/// This trait must be implemented by all event stores in your application.
#[async_trait]
pub trait EventStore: Send + Sync {
    async fn save_events(&mut self, aggregate_id: Uuid, events: &[Event]) -> Result<(), CqrsError>;

    async fn load_events(&self, aggregate_id: Uuid) -> Result<Vec<Event>, CqrsError>;
}
