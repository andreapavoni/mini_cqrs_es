use std::{marker::PhantomData, fmt::Debug};

use async_trait::async_trait;

use crate::{Aggregate, EventStore, EventConsumer, CqrsError, Dispatcher};

pub struct SimpleDispatcher<A, ES>
where
    A: Aggregate,
    ES: EventStore,
{
    event_store: ES,
    event_consumers: Vec<Box<dyn EventConsumer>>,
    marker: PhantomData<A>,
}

impl<A, ES> SimpleDispatcher<A, ES>
where
    A: Aggregate,
    ES: EventStore,
{
    pub fn new(
        event_store: ES,
        event_consumers: Vec<Box<dyn EventConsumer>>,
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
    ES: EventStore<AggregateId = A::Id> + Send + Sync,
    A::Command: Send + Sync,
    A::Event: Debug + Send + Sync,
{
    async fn execute(&mut self, aggregate_id: A::Id, command: A::Command) -> Result<A, CqrsError> {
        let mut aggregate = self.load_aggregate(&aggregate_id).await;

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

    async fn load_aggregate(&self, aggregate_id: &A::Id) -> A {
        match self.event_store.load_events(aggregate_id.clone()).await {
            Ok(events) => {
                let mut aggregate = A::default();
                aggregate.apply_events(&events);
                aggregate
            }
            Err(_) => A::default(),
        }
    }
}
