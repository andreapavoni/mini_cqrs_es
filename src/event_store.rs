use async_trait::async_trait;

use crate::CqrsError;

#[async_trait]
pub trait EventStore {
    type Event: Clone;
    type AggregateId: Clone;

    async fn save_events(
        &mut self,
        aggregate_id: Self::AggregateId,
        events: &[Self::Event],
    ) -> Result<(), CqrsError>;

    async fn load_events(
        &self,
        aggregate_id: Self::AggregateId,
    ) -> Result<Vec<Self::Event>, CqrsError>;
}
