use async_trait::async_trait;

// Event consumer
#[async_trait]
pub trait EventConsumer: Sync + Send {
    type Event;

    async fn process<'a>(&mut self, event: &'a Self::Event);
}
