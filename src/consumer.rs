use async_trait::async_trait;
use std::fmt::Debug; // For Debug bound on M

use crate::{Event, Result};

/// The `EventConsumer` trait defines the behavior of an event consumer, which is responsible for processing events.
///
/// This trait must be implemented by all event consumers in your application.
#[async_trait]
pub trait EventConsumer<M>: Send + Sync + 'static
// Consumer is generic over message type M
where
    M: Send + Debug + 'static,
{
    /// Process an event and return a list of commands (wrapped in type M) to be dispatched.
    async fn process(&mut self, event: &Event) -> Result<Vec<M>>;
    // Context can still be added later if needed:
    // async fn process(&mut self, event: &Event, ctx: &Ctx) -> Result<Vec<M>>;
}

/// The `EventConsumersGroup` trait defines the behavior to process events through multiple consumers.
///
/// This trait is usually implemented when using the `make_event_consumers_group!` macro.
///
/// The individual consumers in the group can be set up as fields within the struct.
///
/// ## Example
///
/// Here's an example of using the `make_event_consumers_group!` macro and later using the created `EventConsumersGroup`:
///
/// ```rust
/// use mini_cqrs_es::{EventConsumer, Event, EventConsumersGroup, make_event_consumers_group};
///
/// struct MyEventConsumer;
///
/// #[async_trait::async_trait]
/// impl EventConsumer for MyEventConsumer {
///     async fn process(&mut self, event: Event) {
///         // Implement the logic to process the event
///         unimplemented!()
///     }
/// }
///
/// make_event_consumers_group! {
///     MyEventConsumersGroup {
///         consumer_one: MyEventConsumer,
///     }
/// }
///
/// // Later in the code where you want to use MyEventConsumersGroup
/// let consumers = MyEventConsumersGroup {
///     consumer_one: MyEventConsumer {},
/// };
/// ```

#[async_trait]
pub trait EventConsumersGroup<M>: Send + Sync + 'static
// Group is generic over message type M
where
    M: Send + Debug + 'static,
{
    /// Process an event through all consumers and collect commands to dispatch.
    async fn process(&mut self, event: &Event) -> Result<Vec<M>>;
    // Context can still be added later if needed
    // async fn process(&mut self, event: &Event, ctx: &Ctx) -> Result<Vec<M>>;
}

/// Macro updated to collect results (Vec<M>) from consumers.
#[macro_export]
macro_rules! make_event_consumers_group {
    (
        $GroupName:ident< $MessageType:ty > { // Added generic message type parameter
            $($Field:ident: $Consumer:ty),* $(,)?
        }
    ) => {
        #[derive(Clone)]
        pub struct $GroupName {
            $(pub $Field: $Consumer,)*
        }

        #[async_trait]
        // Implement trait with the specified message type
        impl $crate::EventConsumersGroup<$MessageType> for $GroupName
        where $MessageType: Send + std::fmt::Debug + 'static // Add necessary bounds for M
        {
            // Updated signature to return Result<Vec<M>>
            async fn process(&mut self, event: &Event) -> $crate::Result<Vec<$MessageType>> {
                // Create futures for each consumer's process method
                // Consumers must implement EventConsumer<M>
                let futures = vec![
                    $(self.$Field.process(event),)*
                ];

                // Use try_join_all to await all futures and propagate the first error
                // This returns Vec<Result<Vec<M>>> effectively, we need Vec<Vec<M>> then flatten
                let results: Vec<Vec<$MessageType>> = futures::future::try_join_all(futures).await?;

                // Flatten the Vec<Vec<M>> into Vec<M>
                let commands_to_dispatch = results.into_iter().flatten().collect();

                Ok(commands_to_dispatch) // Return collected commands
            }

            // TODO: Add context passing if EventConsumer::process adds it
            // async fn process(&mut self, event: &Event, ctx: &Ctx) -> Result<Vec<M>> { ... }
        }
    };
}
