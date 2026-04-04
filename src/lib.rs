//! # MiniCQRS/ES
//!
//! MiniCQRS/ES is a Rust library that simplifies the implementation of the Command-Query
//! Responsibility Segregation (CQRS) architectural pattern in your application.
//!
//! ## Key Features
//!
//! - Provides traits for defining aggregates, commands, and event consumers.
//! - Manages aggregates' state and events handling with optimistic concurrency.
//! - Supports event stores and snapshot stores.
//! - Supports queries on read models.
//! - All trait methods take `&self` for easy concurrent usage.
//! - No `async_trait` dependency — uses native async fn in traits.
//!
//! For more detailed documentation, refer to the specific modules and types provided by MiniCQRS/ES.

pub use ::anyhow;
pub use uuid::Uuid;

mod error;
pub use error::CqrsError;

mod events;
pub use events::{Event, EventPayload, EventStore};

mod aggregate;
pub use aggregate::{
    manager::{AggregateManager, SimpleAggregateManager, SnapshotAggregateManager},
    snapshot::{AggregateSnapshot, SnapshotStore},
    Aggregate,
};

mod command;
pub use command::Command;

mod consumer;
pub use consumer::{EventConsumer, EventConsumers};

mod cqrs;
pub use cqrs::{Cqrs, SimpleCqrs};

mod query;
pub use query::{Query, QueryRunner};

mod repository;
pub use repository::Repository;
