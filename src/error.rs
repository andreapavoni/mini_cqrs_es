use std::error::Error as StdError;
use std::fmt::Display;

/// An error that can occur in a CQRS application.
#[derive(Debug, thiserror::Error)]
pub enum CqrsError {
    /// The aggregate was not found.
    #[error("aggregate not found: {0}")]
    AggregateNotFound(String),

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

    /// A domain/business logic error occurred, preserving the original source error.
    #[error("{0}")]
    DomainSource(#[source] Box<dyn StdError + Send + Sync>),

    /// A command/application invariant was violated before events could be committed.
    #[error("{0}")]
    CommandInvariant(String),

    /// A command/application invariant was violated, preserving the original source error.
    #[error("{0}")]
    CommandInvariantSource(#[source] Box<dyn StdError + Send + Sync>),

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
    pub fn new(message: impl Display) -> Self {
        Self::domain(message)
    }

    /// Creates a domain/business error from any displayable value.
    pub fn domain(error: impl Display) -> Self {
        Self::Domain(error.to_string())
    }

    /// Creates a domain/business error while preserving the original source.
    pub fn domain_source<E>(error: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::DomainSource(Box::new(error))
    }

    /// Creates a command/application invariant error from any displayable value.
    pub fn invariant(error: impl Display) -> Self {
        Self::CommandInvariant(error.to_string())
    }

    /// Creates a command/application invariant error while preserving the original source.
    pub fn invariant_source<E>(error: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::CommandInvariantSource(Box::new(error))
    }
}

impl From<&str> for CqrsError {
    fn from(msg: &str) -> Self {
        Self::domain(msg)
    }
}

impl From<String> for CqrsError {
    fn from(msg: String) -> Self {
        Self::domain(msg)
    }
}
