use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;

use crate::{CqrsError, EventPayload, StoredEvent};

pub mod manager;
pub mod snapshot;

/// The `Aggregate` trait defines the behavior of an aggregate, which represents the state of a
/// domain entity and can be modified by applying events.
///
/// Contains domain state only. The runtime owns identity and optimistic
/// concurrency bookkeeping in [`LoadedAggregate`]. Serialization is required
/// only when using snapshots, not for event-based aggregate recovery.
pub trait Aggregate: Default + Sync + Send {
    /// The type of identity used by this aggregate.
    type Id: Clone + Debug + Display + Eq + Send + Sync + 'static;

    /// The aggregate's complete event family, typically an enum with one variant
    /// per domain fact (for example `CounterEvent::Added` and `CounterEvent::Reset`).
    /// Each `apply` call receives one value from this family; commands return a
    /// vector of such values. This is not restricted to a single event kind.
    type Event: EventPayload + Send + Sync;

    /// The persisted stream namespace, shared by loading, appending, and routing.
    ///
    /// Supply a stable, unique domain name such as `"billing.invoice"`.
    /// Before moving an existing type, pin its previous persisted name explicitly:
    /// changing this value does not rename history or migrate checkpoints.
    /// Types sharing a name share streams and must have compatible ID encodings,
    /// event formats, and domain semantics. Custom stores/routers should use this
    /// method rather than deriving their own name from the Rust type.
    fn aggregate_type() -> &'static str;

    /// Applies an event to the aggregate's state.
    fn apply(&mut self, event: &Self::Event) -> impl Future<Output = ()> + Send;

    /// Applies a sequence of events to the aggregate's state.
    fn apply_events(
        &mut self,
        events: &[StoredEvent],
    ) -> impl Future<Output = Result<(), CqrsError>> + Send {
        async {
            for e in events.iter() {
                self.apply(&e.get_payload::<Self::Event>()?).await;
            }
            Ok(())
        }
    }
}

/// Runtime-owned identity, committed stream version, and domain state.
///
/// Commands receive only [`Self::state`]. Rehydrating managers construct this
/// wrapper; orchestration validates envelopes and advances its version, without
/// asking domain implementations to maintain a second copy of that metadata.
#[derive(Clone, Debug)]
pub struct LoadedAggregate<A: Aggregate> {
    id: A::Id,
    version: u64,
    state: A,
}

impl<A: Aggregate> LoadedAggregate<A> {
    /// Constructs a recovered aggregate. Custom managers must supply state
    /// corresponding to this stream ID and version; use version zero for new state.
    pub fn new(id: A::Id, version: u64, state: A) -> Self {
        Self { id, version, state }
    }

    pub fn id(&self) -> &A::Id {
        &self.id
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn state(&self) -> &A {
        &self.state
    }

    pub fn into_state(self) -> A {
        self.state
    }

    pub(crate) async fn apply_events(&mut self, events: &[StoredEvent]) -> Result<(), CqrsError> {
        let mut version = self.version;
        let id = self.id.to_string();
        // Validate the complete batch before invoking domain application.
        for event in events {
            if event.schema_version == 0
                || event.event_type.is_empty()
                || event.aggregate_type != A::aggregate_type()
                || event.aggregate_id != id
            {
                return Err(CqrsError::EventStore(
                    "event belongs to a different stream".into(),
                ));
            }
            version = version
                .checked_add(1)
                .ok_or_else(|| CqrsError::EventStore("stream version overflow".into()))?;
            if event.version != version {
                return Err(CqrsError::EventStore(format!(
                    "invalid stream version: expected {version}, got {}",
                    event.version,
                )));
            }
        }
        self.state.apply_events(events).await?;
        self.version = version;
        Ok(())
    }
}
