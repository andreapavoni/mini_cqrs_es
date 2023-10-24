//! `mini_cqrs` is a lightweight framework for implementing the Command Query Responsibility Segregation (CQRS) pattern in Rust.
//!
//! CQRS separates the responsibility of reading data (queries) from modifying data (commands), enabling efficient
//! handling of complex domain logic and read model optimizations.
//!
//! This crate provides abstractions and traits for building CQRS-based applications, including aggregates, event
//! stores, dispatchers, event consumers, and queries runners.
//!
//! # Example
//!
//! ```rust
//! use mini_cqrs::{Cqrs, SimpleDispatcher, InMemoryEventStore, SimpleQueriesRunner};
//!
//! // Create an in-memory event store.
//! let event_store = InMemoryEventStore::new();
//!
//! // Create a simple dispatcher and queries runner.
//! let dispatcher = SimpleDispatcher::new(event_store.clone());
//! let queries_runner = SimpleQueriesRunner::new();
//!
//! // Create a Cqrs instance.
//! let mut cqrs = Cqrs::new(dispatcher, queries_runner);
//!
//! // Execute a command.
//! let aggregate_id = "example_aggregate_id".to_string();
//! let command = YourCommand::new(/* command parameters */);
//! let result = cqrs.execute(aggregate_id.clone(), command).await;
//!
//! // Perform a query.
//! let query = YourQuery::new(/* query parameters */);
//! let query_result = cqrs.queries().run(query).await;
//! ```
//!
//! The crate exposes various traits and abstractions that can be implemented to build custom CQRS components for
//! your application.
//!
//! # Modules
//!
//! - `aggregate`: Traits and utilities for defining aggregates.
//! - `consumer`: Traits and utilities for event consumers.
//! - `cqrs`: The central `Cqrs` struct for managing commands and queries.
//! - `dispatcher`: Traits and implementations for dispatchers.
//! - `error`: Error types and utilities.
//! - `events`: Definitions for events and event stores.
//! - `query`: Traits and utilities for querying read models.
//! - `repository`: Traits for defining repositories.
//! - `snapshot`: Traits and utilities for aggregate snapshots.
//!
//! # License
//!
//! This crate is open-source and available under the MIT License. See the [LICENSE](https://github.com/andreapavoni/mini_cqrs/blob/main/LICENSE) file for more details.
//!
//! # Contributing
//!
//! Contributions and bug reports are welcome! Feel free to open issues or create pull requests on the [GitHub repository](https://github.com/andreapavoni/mini_cqrs).

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
