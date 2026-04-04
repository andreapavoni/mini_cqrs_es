use std::future::Future;
use std::pin::Pin;

use crate::Event;

/// The `EventConsumer` trait defines the behavior of an event consumer, which is responsible
/// for processing events (e.g., updating read models, sending notifications).
///
/// Methods take `&self` to allow concurrent access.
pub trait EventConsumer: Send + Sync {
    fn process(&self, event: &Event) -> impl Future<Output = ()> + Send;
}

// Internal dyn-compatible wrapper so we can store consumers in a Vec<Box<dyn ...>>.
trait DynEventConsumer: Send + Sync {
    fn process_dyn<'a>(
        &'a self,
        event: &'a Event,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

impl<T: EventConsumer> DynEventConsumer for T {
    fn process_dyn<'a>(
        &'a self,
        event: &'a Event,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(EventConsumer::process(self, event))
    }
}

/// A collection of event consumers that processes events through all of them.
///
/// Use the builder pattern to add consumers:
///
/// ```rust,ignore
/// let consumers = EventConsumers::new()
///     .with(MyConsumer::new())
///     .with(LoggingConsumer {});
/// ```
pub struct EventConsumers {
    consumers: Vec<Box<dyn DynEventConsumer>>,
}

impl EventConsumers {
    pub fn new() -> Self {
        Self {
            consumers: Vec::new(),
        }
    }

    /// Adds a consumer to the group.
    pub fn with(mut self, consumer: impl EventConsumer + 'static) -> Self {
        self.consumers.push(Box::new(consumer));
        self
    }

    /// Processes an event through all consumers sequentially.
    pub async fn process(&self, event: &Event) {
        for consumer in &self.consumers {
            consumer.process_dyn(event).await;
        }
    }
}

impl Default for EventConsumers {
    fn default() -> Self {
        Self::new()
    }
}
