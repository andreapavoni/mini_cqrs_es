// In mini_cqrs_es library (e.g., src/error.rs)
use thiserror::Error;
use uuid::Uuid; // Assuming Uuid is used elsewhere

#[derive(Error, Debug)]
pub enum CqrsError {
    #[error("Failed to deserialize event payload: {0}")]
    PayloadDeserialization(#[from] serde_json::Error), // Auto-wrap serde_json::Error

    #[error("Event store operation failed: {0}")]
    StoreOperation(String), // Example for store errors

    #[error("Concurrency conflict for aggregate {aggregate_id}: expected version {expected}, found {actual}")]
    Concurrency {
        aggregate_id: Uuid,
        expected: u64,
        actual: u64,
    },

    #[error("Aggregate '{0}' not found")]
    AggregateNotFound(Uuid),

    #[error("Snapshot error: {0}")]
    Snapshot(String),

    #[error("Generic error: {0}")]
    Generic(#[from] anyhow::Error), // Can wrap anyhow if needed elsewhere

                                    // Add other specific error variants as needed
}

// Make it compatible with functions returning anyhow::Error if desired
// impl From<CqrsError> for anyhow::Error { ... }
