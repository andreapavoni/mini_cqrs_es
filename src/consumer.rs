use async_trait::async_trait;

use crate::Event;

// Event consumer
#[async_trait]
pub trait EventConsumer: Send + Sync + 'static {
    async fn process(&mut self, event: Event);
}


#[async_trait]
pub trait EventConsumersGroup: Send + Sync + 'static {
    async fn process<'a>(&mut self, event: &'a Event);
}

// event_consumers_group!! {
//     SomeEnum {
//         VariantOne => Struct1,
//         VariantTwo => Struct2,
//     }
// }

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
            async fn process<'a>(&mut self, e: &'a Event) {
                match self {
                    $(
                        $Name::$Variant(c) => {
                            c.process(e.clone()).await;
                        }
                    )*
                };
            }
        }
    };
}

#[allow(unused)]
pub(crate) use event_consumers_group;
