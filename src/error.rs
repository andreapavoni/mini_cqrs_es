use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum CqrsError {
    #[error("Failed to deserialize event payload: {0}")]
    PayloadDeserialization(#[from] serde_json::Error),

    #[error("Event store operation failed for aggregate {aggregate_id}: {source}")]
    StoreOperation {
        aggregate_id: Uuid,
        #[source]
        source: anyhow::Error, // Source is anyhow::Error
    },

    #[error("Concurrency conflict for aggregate {aggregate_id}: expected version {expected}, found {actual}")]
    Concurrency {
        aggregate_id: Uuid,
        expected: u64,
        actual: u64,
    },

    #[error("Aggregate '{0}' not found")]
    AggregateNotFound(Uuid),

    #[error("Command validation failed for aggregate {aggregate_id}: {reason}")]
    CommandValidation { aggregate_id: Uuid, reason: String },

    #[error("Snapshot operation failed: {0}")]
    Snapshot(String), // Or wrap specific snapshot errors

    #[error("Command dispatch failed: {0}")]
    CommandDispatch(String),

    // Changed Generic to wrap anyhow::Error directly using #[from]
    #[error("Generic CQRS error: {0}")]
    Generic(#[from] anyhow::Error),
}

// Result alias within the library
pub type Result<T, E = CqrsError> = std::result::Result<T, E>;
