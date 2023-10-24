use async_trait::async_trait;

use crate::Event;

/// A trait that defines the behavior of an event consumer.
///
/// An event consumer is responsible for processing events.
///
/// This trait must be implemented by all event consumers in your application.
#[async_trait]
pub trait EventConsumer: Send + Sync + 'static {
    async fn process(&mut self, event: Event);
}

/// A trait that defines the behavior of an event consumers group.
///
/// An event consumers group handles multiple event consumers. It is implemented through `event_consumers_group!`.
///
#[async_trait]
pub trait EventConsumersGroup: Send + Sync + 'static {
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
