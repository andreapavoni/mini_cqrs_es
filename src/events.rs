use std::fmt::Display;
use std::future::Future;

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Serialize};

use crate::{CqrsError, Uuid};

/// The `Event` struct represents a change to the state of an aggregate in a CQRS application.
///
/// An event contains information such as its unique ID, event type, aggregate ID, payload data,
/// version, and timestamp. Events are created by the framework when a command is executed,
/// wrapping domain event payloads.
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
    /// Creates a new event from an event payload and aggregate ID.
    pub fn new<T: EventPayload>(
        aggregate_id: Uuid,
        payload: T,
        version: u64,
    ) -> Result<Self, CqrsError> {
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            event_type: payload.name(),
            aggregate_id,
            payload: serde_json::to_value(payload)?,
            version,
            timestamp: Utc::now(),
        })
    }

    /// Gets the payload of the event.
    pub fn get_payload<T: EventPayload>(&self) -> Result<T, CqrsError> {
        Ok(serde_json::from_value(self.payload.clone())?)
    }
}

/// The `EventPayload` trait defines the behavior of an event payload, representing the change
/// the event made to the state of an aggregate.
///
/// Event payloads are pure domain events. The aggregate ID is not part of the payload — it is
/// managed by the framework via the `Event` wrapper.
pub trait EventPayload: Serialize + DeserializeOwned + Clone + Display {
    /// Gets the name of the event payload. Defaults to the `Display` representation.
    fn name(&self) -> String {
        self.to_string()
    }
}

/// The `EventStore` trait defines the behavior for storing and loading events,
/// allowing the application to keep a historical record of state changes.
///
/// All methods take `&self` to allow concurrent access. Implementations should
/// use interior mutability (e.g., `RwLock`, `Mutex`) or connection pools.
///
/// The `expected_version` parameter on `save_events` enables optimistic concurrency control.
/// Pass `0` for a new aggregate. If the actual version doesn't match, return `CqrsError::Conflict`.
pub trait EventStore: Send + Sync {
    /// Saves events to the event store. `expected_version` is the last known version of the aggregate.
    fn save_events(
        &self,
        aggregate_id: Uuid,
        events: &[Event],
        expected_version: u64,
    ) -> impl Future<Output = Result<(), CqrsError>> + Send;

    /// Loads events from the event store. Returns the events and the current version.
    fn load_events(
        &self,
        aggregate_id: Uuid,
    ) -> impl Future<Output = Result<(Vec<Event>, u64), CqrsError>> + Send;
}
