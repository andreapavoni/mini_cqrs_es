//! # MiniCQRS/ES
//!
//! Define domain state ([`Aggregate`]), facts ([`EventPayload`]), and requests
//! ([`Command`]). [`SimpleCqrs::new`] shares one [`EventLog`] for recovery and
//! append; add a [`Projector`] to maintain application read models.
//!
//! Toasty is re-exported as [`toasty`] for your own read models and queries.
//! [`ReadModels::builder`] registers application models plus library-managed
//! checkpoints. Event, snapshot and checkpoint implementation models are not
//! part of the normal setup. [`ReadModels::database`] exposes native query access.
//!
//! Choose a database driver through explicit features, then connect with URLs
//! or native Toasty database handles. The library has no default database driver
//! and performs no filesystem operations. `create_schema` is explicit setup for
//! fresh, provisioned databases; connection alone never changes tables.
//!
//! Events can commit before projection or snapshot processing fails. Inspect
//! [`ExecutionError`] before retrying, and use [`Projector::catch_up`] to repair
//! read-side delivery. [`ReplayPlan`] supports bounded offline rebuilding.
//!
//! Start with `examples/hotel/` and the README for the complete workflow.

pub use ::anyhow;
pub use ::toasty;

mod read_models;
pub use read_models::{ReadModels, ReadModelsBuilder};

pub mod persistence;
pub use persistence::{EventLog, SnapshotCache};

mod error;
pub use error::CqrsError;

mod events;
pub use events::{EventMetadata, EventPayload, EventStore, NewEvent, StoredEvent, StreamKey};

mod aggregate;
pub use aggregate::{
    Aggregate, LoadedAggregate,
    manager::{AggregateManager, SimpleAggregateManager, SnapshotAggregateManager},
    snapshot::{AggregateSnapshot, SnapshotStore},
};

mod command;
pub use command::Command;

mod consumer;
pub use consumer::{EventConsumer, EventConsumers};

mod projection;
pub use projection::{Projection, ProjectionCheckpoint, Projector};

mod replay;
pub use replay::{
    DEFAULT_PAGE_SIZE, EventStreamReader, PreparedReplay, ReplayError, ReplayPlan, ReplayProgress,
    replay_from, replay_stream,
};

mod cqrs;
pub use cqrs::{Cqrs, ExecutionError, ExecutionReceipt, PostCommitStage, SimpleCqrs};

mod query;
pub use query::{Query, QueryRunner};
