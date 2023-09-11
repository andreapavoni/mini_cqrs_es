pub mod simple;
pub mod snapshot;

pub use simple::SimpleDispatcher;
pub use snapshot::SnapshotDispatcher;

use async_trait::async_trait;

use crate::{Aggregate, CqrsError, EventStore};

// Command dispatcher
#[async_trait]
pub trait Dispatcher<A, ES>: Send
where
    A: Aggregate,
    ES: EventStore,
{
    async fn execute(&mut self, aggregate_id: A::Id, command: A::Command) -> Result<A, CqrsError>;

    async fn load_aggregate(&self, aggregate_id: &A::Id) -> A;
}
