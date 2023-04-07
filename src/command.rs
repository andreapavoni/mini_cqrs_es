use std::{fmt::Debug, marker::PhantomData};

use async_trait::async_trait;

use crate::{Aggregate, EventStore, CqrsError, EventConsumer};

// Command dispatcher
#[async_trait]
pub trait Dispatcher<A, ES>: Send
where
    A: Aggregate,
    ES: EventStore<Event = A::Event>,
{
    async fn execute(&mut self, aggregate_id: A::Id, command: A::Command) -> Result<A, CqrsError>;
}

pub struct SimpleDispatcher<A, ES>
where
    A: Aggregate,
    ES: EventStore<Event = A::Event>,
{
    event_store: ES,
    event_consumers: Vec<Box<dyn EventConsumer<Event = A::Event>>>,
    marker: PhantomData<A>,
}

impl<A, ES> SimpleDispatcher<A, ES>
where
    A: Aggregate,
    ES: EventStore<Event = A::Event>,
{
    pub fn new(
        event_store: ES,
        event_consumers: Vec<Box<dyn EventConsumer<Event = A::Event>>>,
    ) -> Self {
        Self {
            event_store,
            event_consumers,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<A, ES> Dispatcher<A, ES> for SimpleDispatcher<A, ES>
where
    A: Aggregate,
    ES: EventStore<Event = A::Event, AggregateId = A::Id> + Send + Sync,
    A::Command: Send + Sync,
    A::Event: Debug + Send + Sync,
{
    async fn execute(&mut self, aggregate_id: A::Id, command: A::Command) -> Result<A, CqrsError> {
        let mut aggregate = match self.event_store.load_events(aggregate_id.clone()).await {
            Ok(events) => {
                let mut aggregate = A::default();
                aggregate.apply_events(&events);
                aggregate
            }
            Err(_) => A::default(),
        };

        aggregate.set_aggregate_id(aggregate_id.clone());

        let events = aggregate.handle(command).await?;
        self.event_store.save_events(aggregate_id, &events).await?;

        aggregate.apply_events(&events);

        for consumer in self.event_consumers.iter_mut() {
            for event in &events {
                consumer.process(event).await;
            }
        }

        Ok(aggregate)
    }
}
