/// A CQRS library for Rust.
///
/// This library provides a simple and easy-to-use way to implement a CQRS architecture in your Rust application.
mod aggregate;
mod command;
mod consumer;
mod cqrs;
mod error;
mod events;
mod query;
mod repository;

pub use aggregate::{
    manager::{AggregateManager, SimpleAggregateManager, SnapshotAggregateManager},
    snapshot::{AggregateSnapshot, SnapshotStore},
    Aggregate,
};

pub use command::Command;
pub use consumer::{EventConsumer, EventConsumersGroup};
pub use cqrs::Cqrs;
pub use error::CqrsError;
pub use events::{Event, EventPayload, EventStore};
pub use query::{ModelReader, QueriesRunner, Query};
pub use repository::Repository;
