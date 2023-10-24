use crate::{
    Aggregate, AggregateManager, CqrsError, EventConsumersGroup, EventStore, QueriesRunner,
};
use uuid::Uuid;

pub struct Cqrs<ES, Q, C, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    C: EventConsumersGroup,
    Q: QueriesRunner,
{
    aggregate_manager: AM,
    event_store: ES,
    consumers: Vec<C>,
    queries: Q,
}

impl<ES, Q, C, AM> Cqrs<ES, Q, C, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    C: EventConsumersGroup,
    Q: QueriesRunner,
{
    pub fn new(aggregate_manager: AM, event_store: ES, consumers: Vec<C>, queries: Q) -> Self {
        Self {
            aggregate_manager,
            event_store,
            consumers,
            queries,
        }
    }

    pub async fn execute<A>(
        &mut self,
        aggregate_id: Uuid,
        command: A::Command,
    ) -> Result<(), CqrsError>
    where
        A: Aggregate + Clone,
    {
        let mut aggregate = self.aggregate_manager.load::<A>(aggregate_id).await?;

        let events = aggregate.handle(command).await?;

        self.event_store.save_events(aggregate_id, &events).await?;
        aggregate.apply_events(&events);

        for consumer in self.consumers.iter_mut() {
            for event in events.iter() {
                consumer.process(&event).await;
            }
        }

        self.aggregate_manager.store::<A>(&aggregate).await?;

        Ok(())
    }

    pub fn queries(&self) -> &Q {
        &self.queries
    }

    pub fn event_queries(&self) -> &ES {
        &self.event_store
    }
}
