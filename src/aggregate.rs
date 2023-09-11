use std::fmt::Debug;

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use crate::{CqrsError, EventPayload, Event};

// Aggregate
#[async_trait]
pub trait Aggregate: Debug + Default + Sync + Send + Serialize + DeserializeOwned {
    type Command;
    type Event: EventPayload;
    type Id: Clone + Send + Sync + Debug + ToString;

    async fn handle(&self, command: Self::Command) -> Result<Vec<Event>, CqrsError>;
    fn apply(&mut self, event: &Self::Event);
    fn aggregate_id(&self) -> Self::Id;
    fn set_aggregate_id(&mut self, id: Self::Id);

    fn apply_events(&mut self, events: &[Event]) {
        for e in events.iter() {
            self.apply(&e.get_payload::<Self::Event>());
        }
    }
}

