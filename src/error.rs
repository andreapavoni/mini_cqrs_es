use crate::Uuid;

/// An error that can occur in a CQRS application.
#[derive(Debug, thiserror::Error)]
pub enum CqrsError {
    /// The aggregate was not found.
    #[error("aggregate not found: {0}")]
    AggregateNotFound(Uuid),

    /// An error occurred in the event store.
    #[error("event store error: {0}")]
    EventStore(String),

    /// A serialization/deserialization error occurred.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// A snapshot store error occurred.
    #[error("snapshot store error: {0}")]
    SnapshotStore(String),

    /// A domain/business logic error occurred.
    #[error("{0}")]
    Domain(String),

    /// A concurrency conflict occurred (optimistic locking).
    #[error("concurrency conflict: expected version {expected_version}, got {actual_version}")]
    Conflict {
        expected_version: u64,
        actual_version: u64,
    },

    /// Any other error, with full `anyhow` context and backtrace support.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl CqrsError {
    /// Creates a domain error. Convenience constructor.
    pub fn new(message: String) -> Self {
        Self::Domain(message)
    }
}

impl From<&str> for CqrsError {
    fn from(msg: &str) -> Self {
        Self::Domain(msg.to_string())
    }
}

impl From<String> for CqrsError {
    fn from(msg: String) -> Self {
        Self::Domain(msg)
    }
}
