use async_trait::async_trait;

use crate::Event;

// Event consumer
#[async_trait]
pub trait EventConsumer: Sync + Send {
    async fn process<'a>(&mut self, event: &'a Event);
}
