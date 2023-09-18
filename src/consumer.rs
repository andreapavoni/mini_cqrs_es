use async_trait::async_trait;

use crate::Event;

/// A trait for event consumers in the event sourcing system.
///
/// Event consumers are responsible for processing events as they occur. Implement this trait
/// to define custom event processing logic.
///
/// # Example
///
/// ```rust
/// use async_trait::async_trait;
/// use mini_cqrs::Event;
///
/// struct YourEventConsumer;
///
/// #[async_trait]
/// impl EventConsumer for YourEventConsumer {
///     async fn process(&mut self, event: Event) {
///         // Implement event processing logic here.
///         println!("Processing event: {:?}", event);
///     }
/// }
/// ```
#[async_trait]
pub trait EventConsumer: Send + Sync + 'static {
    /// Processes an event.
    ///
    /// # Parameters
    ///
    /// - `event`: The event to be processed.
    async fn process(&mut self, event: Event);
}

/// A trait for groups of event consumers.
///
/// This trait allows you to define groups of event consumers that can be processed together.
/// Implement this trait to create higher-level event processing logic that involves multiple
/// consumers.
///
/// # Example
///
/// ```rust
/// use async_trait::async_trait;
/// use mini_cqrs::{Event, EventConsumer, EventConsumersGroup};
///
/// struct YourEventConsumer1;
/// struct YourEventConsumer2;
///
/// struct YourEventGroup {
///     consumer1: YourEventConsumer1,
///     consumer2: YourEventConsumer2,
/// }
///
/// #[async_trait]
/// impl EventConsumer for YourEventConsumer1 {
///     async fn process(&mut self, event: Event) {
///         // Implement event processing logic for consumer 1.
///         println!("Consumer 1 processing event: {:?}", event);
///     }
/// }
///
/// #[async_trait]
/// impl EventConsumer for YourEventConsumer2 {
///     async fn process(&mut self, event: Event) {
///         // Implement event processing logic for consumer 2.
///         println!("Consumer 2 processing event: {:?}", event);
///     }
/// }
///
/// #[async_trait]
/// impl EventConsumersGroup for YourEventGroup {
///     async fn process(&mut self, event: &Event) {
///         // Implement group-level event processing logic.
///         self.consumer1.process(event.clone()).await;
///         self.consumer2.process(event.clone()).await;
///     }
/// }
/// ```
#[async_trait]
pub trait EventConsumersGroup: Send + Sync + 'static {
    /// Processes an event within the group.
    ///
    /// # Parameters
    ///
    /// - `event`: The event to be processed.
    async fn process(&mut self, event: &Event);
}

/// A macro for defining groups of event consumers.
///
/// This macro simplifies the creation of enum-based event consumer groups.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::{EventConsumer, EventConsumersGroup, event_consumers_group};
///
/// struct YourEventConsumer1;
/// struct YourEventConsumer2;
///
/// event_consumers_group! {
///     YourEventGroup {
///         VariantOne => YourEventConsumer1,
///         VariantTwo => YourEventConsumer2,
///     }
/// }
/// ```
#[macro_export]
macro_rules! event_consumers_group {
    (
        $Name:ident {
            $($Variant:ident => $f:ident),* $(,)?
        }
    ) => {
        #[derive(Clone)]
        pub enum $Name {
            $($Variant($f),)*
        }

        #[async_trait]
        impl EventConsumersGroup for $Name {
            async fn process(&mut self, event: &Event) {
                match self {
                    $(
                        $Name::$Variant(c) => {
                            c.process(event.clone()).await;
                        }
                    )*
                };
            }
        }
    };
}

/// A macro for wrapping event consumers in a group.
///
/// This macro simplifies the implementation of event consumer groups by generating the necessary
/// code for grouping multiple event consumers.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::{EventConsumer, event_consumers_group, wrap_event_consumers};
///
/// struct YourEventConsumer1;
/// struct YourEventConsumer2;
///
/// event_consumers_group! {
///     YourEventGroup {
///         VariantOne => YourEventConsumer1,
///         VariantTwo => YourEventConsumer2,
///     }
/// }
///
/// wrap_event_consumers! {
///     YourEventGroup => [
///         VariantOne,
///         VariantTwo,
///     ]
/// }
/// ```
#[macro_export]
macro_rules! wrap_event_consumers {
    ($GroupName:ident => [$($Variant:ident),* $(,)?]) => {
        $(
            impl From<$GroupName> for $Variant {
                fn from(group: $GroupName) -> Self {
                    match group {
                        $GroupName::$Variant(consumer) => consumer,
                        _ => unreachable!(),
                    }
                }
            }

            impl Into<$GroupName> for $Variant {
                fn into(self) -> $GroupName {
                    $GroupName::$Variant(self)
                }
            }
        )*
    };
}
