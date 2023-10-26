use async_trait::async_trait;

use crate::Event;

/// The `EventConsumer` trait defines the behavior of an event consumer, which is responsible for processing events.
///
/// This trait must be implemented by all event consumers in your application.
#[async_trait]
pub trait EventConsumer: Send + Sync + 'static {
    async fn process(&mut self, event: Event);
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
pub trait EventConsumersGroup: Send + Sync + 'static {
    async fn process(&mut self, event: &Event);
}

/// A macro that simplifies the creation of a type that implements the `EventConsumersGroup` trait.
#[macro_export]
macro_rules! make_event_consumers_group {
    (
        $GroupName:ident {
            $($Field:ident: $Consumer:ident),* $(,)?
        }
    ) => {
        #[derive(Clone)]
        pub struct $GroupName {
            $(pub $Field: $Consumer,)*
        }

        #[async_trait]
        impl EventConsumersGroup for $GroupName {
            async fn process(&mut self, event: &Event) {
                $(let $Field = self.$Field.process(event.clone());)*
                futures::join!($($Field,)*);
            }
        }
    };
}
