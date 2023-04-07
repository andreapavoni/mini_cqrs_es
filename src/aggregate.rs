use std::fmt::Debug;

use async_trait::async_trait;

use crate::CqrsError;

// Aggregate
#[async_trait]
pub trait Aggregate: Debug + Default + Sync + Send {
    type Command;
    type Event;
    type Id: Clone + Send + Sync + Debug;

    async fn handle(&self, command: Self::Command) -> Result<Vec<Self::Event>, CqrsError>;
    fn apply(&mut self, event: &Self::Event);
    fn aggregate_id(&self) -> Self::Id;
    fn set_aggregate_id(&mut self, id: Self::Id);

    fn apply_events(&mut self, events: &[Self::Event]) {
        for e in events.iter() {
            self.apply(e);
        }
    }
}
