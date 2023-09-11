use async_trait::async_trait;

use crate::{CqrsError, Event};

#[async_trait]
pub trait EventStore {
    type AggregateId: Clone;

    async fn save_events(
        &mut self,
        aggregate_id: Self::AggregateId,
        events: &[Event],
    ) -> Result<(), CqrsError>;

    async fn load_events(
        &self,
        aggregate_id: Self::AggregateId,
    ) -> Result<Vec<Event>, CqrsError>;
}
