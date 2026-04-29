use std::collections::HashMap;
use std::fmt::Display;
use std::future::Future;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::CqrsError;

/// Optional metadata associated with an event.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EventMetadata {
    pub command_id: Option<String>,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub actor: Option<String>,
    pub tenant_id: Option<String>,
    #[serde(default)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// A new event to be persisted by the event store.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewEvent {
    pub event_type: String,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub metadata: EventMetadata,
    pub timestamp: DateTime<Utc>,
}

impl NewEvent {
    pub fn from_payload<T: EventPayload>(
        payload: T,
        metadata: EventMetadata,
    ) -> Result<Self, CqrsError> {
        Ok(Self {
            event_type: payload.name(),
            payload: serde_json::to_value(payload)?,
            metadata,
            timestamp: Utc::now(),
        })
    }
}

/// A persisted event envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredEvent {
    pub id: String,
    pub aggregate_id: String,
    pub aggregate_type: String,
    pub version: u64,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub metadata: EventMetadata,
    pub global_sequence: Option<i64>,
    pub timestamp: DateTime<Utc>,
}

impl StoredEvent {
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
        aggregate_type: &str,
        aggregate_id: &str,
        events: &[NewEvent],
        expected_version: u64,
    ) -> impl Future<Output = Result<Vec<StoredEvent>, CqrsError>> + Send;

    /// Loads events from the event store. Returns the events and the current version.
    fn load_events(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> impl Future<Output = Result<(Vec<StoredEvent>, u64), CqrsError>> + Send;
}
