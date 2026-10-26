use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{CqrsError, DEFAULT_PAGE_SIZE, EventStreamReader};

/// Encoded stream identity: `(aggregate_type, aggregate_id)`.
/// Discovery cursors use the store's ordering/collation, not Rust string ordering.
pub type StreamKey = (String, String);

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
    pub schema_version: u32,
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
        if T::SCHEMA_VERSION == 0 || payload.event_type().is_empty() {
            return Err(CqrsError::EventFormat(
                "event kind and positive schema revision are required".into(),
            ));
        }
        Ok(Self {
            event_type: payload.event_type().to_owned(),
            schema_version: T::SCHEMA_VERSION,
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
    pub schema_version: u32,
    pub payload: serde_json::Value,
    pub metadata: EventMetadata,
    pub global_sequence: Option<i64>,
    pub timestamp: DateTime<Utc>,
}

impl StoredEvent {
    pub fn get_payload<T: EventPayload>(&self) -> Result<T, CqrsError> {
        if self.schema_version == 0 || self.event_type.is_empty() {
            return Err(CqrsError::EventFormat("invalid event kind/revision".into()));
        }
        T::decode(&self.event_type, self.schema_version, self.payload.clone())
    }
}

/// The `EventPayload` trait defines the behavior of an event payload, representing the change
/// the event made to the state of an aggregate.
///
/// Event payloads are pure domain events. The aggregate ID is not part of the payload — it is
/// managed by the framework via the `StoredEvent` envelope.
pub trait EventPayload: Serialize + DeserializeOwned {
    /// Current revision written by `NewEvent::from_payload`; must be positive.
    const SCHEMA_VERSION: u32 = 1;

    /// Stable persisted kind for this variant, independent of display text.
    fn event_type(&self) -> &'static str;

    /// Decode a persisted event. Override to explicitly upgrade historical JSON,
    /// then call `decode_current` to validate the resulting variant's kind.
    /// Unknown kinds/revisions must fail; never silently assume a current format.
    fn decode(kind: &str, revision: u32, payload: serde_json::Value) -> Result<Self, CqrsError> {
        if revision == 0 || revision != Self::SCHEMA_VERSION {
            return Err(CqrsError::EventFormat(format!(
                "unsupported event revision {revision} for {kind}"
            )));
        }
        Self::decode_current(kind, payload)
    }

    /// Decode current JSON and verify its variant agrees with the envelope kind.
    fn decode_current(kind: &str, payload: serde_json::Value) -> Result<Self, CqrsError> {
        let decoded: Self = serde_json::from_value(payload)?;
        if kind.is_empty() || decoded.event_type() != kind {
            return Err(CqrsError::EventFormat(format!(
                "event kind mismatch: envelope {kind}, decoded {}",
                decoded.event_type()
            )));
        }
        Ok(decoded)
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
    /// A successful result confirms the returned events were committed. A
    /// `Conflict` rejects the append without committing its events. Other errors
    /// do not necessarily establish rollback (for example, a lost commit response).
    fn save_events(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        events: &[NewEvent],
        expected_version: u64,
    ) -> impl Future<Output = Result<Vec<StoredEvent>, CqrsError>> + Send;

    /// Returns the current committed stream tip, or zero for an absent stream.
    fn stream_version(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> impl Future<Output = Result<u64, CqrsError>> + Send;

    /// Reads at most `limit` events in `(after_version, through_version]`, ordered
    /// by version. Bounds must satisfy `after_version <= through_version`.
    /// For intact history through that endpoint, return exactly the smaller of
    /// `limit` and the remaining version count. Never replace the endpoint with
    /// a newer tip. Readers validate completeness, identity, and ordering.
    fn load_events_after(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        after_version: u64,
        through_version: u64,
        limit: NonZeroUsize,
    ) -> impl Future<Output = Result<Vec<StoredEvent>, CqrsError>> + Send;

    /// Seeks nonempty streams strictly after a prior returned key. Results are
    /// unique and ordered by type/ID using one consistent database collation;
    /// return up to `limit` keys, or empty on exhaustion. Include streams even
    /// when their first event is missing, so rebuilding can detect corruption.
    /// Pause writers for complete discovery; pages are not a global snapshot.
    fn list_streams_after(
        &self,
        after: Option<&StreamKey>,
        limit: NonZeroUsize,
    ) -> impl Future<Output = Result<Vec<StreamKey>, CqrsError>> + Send;

    /// Collects a complete stream in memory, using validated bounded reads.
    /// Prefer [`EventStreamReader`] for recovery/replay of large histories.
    fn load_events(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> impl Future<Output = Result<(Vec<StoredEvent>, u64), CqrsError>> + Send {
        async move {
            let mut reader =
                EventStreamReader::open(self, aggregate_type, aggregate_id, 0, DEFAULT_PAGE_SIZE)
                    .await?;
            let mut events = Vec::new();
            while let Some(page) = reader.next_page().await? {
                events.extend(page);
            }
            Ok((events, reader.through_version()))
        }
    }

    /// Collects all stream keys in memory. Bounded rebuilds should iterate
    /// [`Self::list_streams_after`] instead.
    fn list_streams(&self) -> impl Future<Output = Result<Vec<StreamKey>, CqrsError>> + Send {
        async move {
            let mut streams = Vec::new();
            let mut after = None;
            loop {
                let page = self
                    .list_streams_after(after.as_ref(), DEFAULT_PAGE_SIZE)
                    .await?;
                if page.is_empty() {
                    break;
                }
                if page.len() > DEFAULT_PAGE_SIZE.get() || page.last() == after.as_ref() {
                    return Err(CqrsError::EventStore(
                        "invalid stream discovery page".into(),
                    ));
                }
                after = page.last().cloned();
                streams.extend(page);
            }
            Ok(streams)
        }
    }
}

impl<T: EventStore + ?Sized> EventStore for std::sync::Arc<T> {
    async fn save_events(
        &self,
        kind: &str,
        id: &str,
        events: &[NewEvent],
        expected: u64,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        (**self).save_events(kind, id, events, expected).await
    }
    async fn stream_version(&self, kind: &str, id: &str) -> Result<u64, CqrsError> {
        (**self).stream_version(kind, id).await
    }
    async fn load_events_after(
        &self,
        kind: &str,
        id: &str,
        after: u64,
        through: u64,
        limit: NonZeroUsize,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        (**self)
            .load_events_after(kind, id, after, through, limit)
            .await
    }
    async fn list_streams_after(
        &self,
        after: Option<&StreamKey>,
        limit: NonZeroUsize,
    ) -> Result<Vec<StreamKey>, CqrsError> {
        (**self).list_streams_after(after, limit).await
    }
    async fn load_events(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<(Vec<StoredEvent>, u64), CqrsError> {
        (**self).load_events(kind, id).await
    }
    async fn list_streams(&self) -> Result<Vec<StreamKey>, CqrsError> {
        (**self).list_streams().await
    }
}
