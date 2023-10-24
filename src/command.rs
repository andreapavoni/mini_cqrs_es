use async_trait::async_trait;

use crate::{Aggregate, CqrsError, EventPayload, Event};

#[async_trait]
pub trait Command {
    type Aggregate: Aggregate<Event = Self::Event>;
    type Event: EventPayload;

    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<Event>, CqrsError>;
}
