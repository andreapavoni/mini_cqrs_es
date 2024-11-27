//! # MiniCQRS/ES
//!
//! MiniCQRS/ES is a Rust library that simplifies the implementation of the Command-Query Responsibility Segregation (CQRS) architectural pattern in your application.
//!
//! ## Key Features
//!
//! - Provides traits for defining aggregates, commands, and event consumers.
//! - Manages aggregates' state and events handling.
//! - Supports event stores and snapshot stores.
//! - Supports queries on read models.
//! - Extreme flexibility and extensibility by implementing traits.
//!
//! For more detailed documentation, refer to the specific modules and types provided by MiniCQRS/ES.

mod aggregate;
mod command;
mod consumer;
mod cqrs;
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
pub use events::{Event, EventPayload, EventStore};
pub use query::{ModelReader, QueriesRunner, Query};
pub use repository::Repository;
pub use uuid::Uuid;
