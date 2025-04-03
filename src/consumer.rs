use async_trait::async_trait;
use std::fmt::Debug; // For Debug bound on M

use crate::{Event, Result};

/// The `EventConsumer` trait defines the behavior of an event consumer, which is responsible for processing events.
///
/// This trait must be implemented by all event consumers in your application.

#[async_trait]
pub trait EventConsumer<M, Ctx = ()>: Send + Sync + 'static
where
    M: Send + Debug + 'static,
    Ctx: Send + Sync + 'static,
{
    /// Process an event, potentially using context, and return commands to dispatch.
    async fn process(&mut self, event: &Event, ctx: &Ctx) -> Result<Vec<M>>;
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
pub trait EventConsumersGroup<M, Ctx = ()>: Send + Sync + 'static
where
    M: Send + Debug + 'static,
    Ctx: Send + Sync + 'static,
{
    /// Process an event through all consumers, passing context, and collect commands.
    async fn process(&mut self, event: &Event, ctx: &Ctx) -> Result<Vec<M>>;
}

#[macro_export]
macro_rules! make_event_consumers_group {
    (
        $GroupName:ident< $MessageType:ty, $ContextType:ty > {
            $($Field:ident: $Consumer:ty),* $(,)?
        }
    ) => {
        #[derive(Clone)]
        pub struct $GroupName {
            $(pub $Field: $Consumer,)*
        }

        #[async_trait]
        // Implement trait with the specified message and context types
        impl $crate::EventConsumersGroup<$MessageType, $ContextType> for $GroupName
        where
            $MessageType: Send + std::fmt::Debug + 'static,
            $ContextType: Send + Sync + 'static, // Add bounds for Ctx
            // Ensure consumers implement the correct trait with context
            $($Consumer: $crate::EventConsumer<$MessageType, $ContextType>,)*
        {
            // Updated signature to accept context
            async fn process(&mut self, event: &Event, ctx: &$ContextType) -> $crate::Result<Vec<$MessageType>> {
                // Create futures for each consumer's process method, passing context
                let futures = vec![
                    // Pass context reference to each consumer's process method
                    $(self.$Field.process(event, ctx),)*
                ];

                // Use try_join_all to await all futures and propagate the first error
                let results: Vec<Vec<$MessageType>> = futures::future::try_join_all(futures).await?;

                // Flatten the Vec<Vec<M>> into Vec<M>
                let commands_to_dispatch = results.into_iter().flatten().collect();

                Ok(commands_to_dispatch) // Return collected commands
            }
        }
    };
}
