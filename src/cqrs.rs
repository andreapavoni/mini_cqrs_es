use crate::{
    Aggregate, AggregateManager, CqrsError, EventConsumersGroup, EventStore, QueriesRunner, Command,
};
use uuid::Uuid;

pub struct Cqrs<ES, Q, EC, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    EC: EventConsumersGroup,
    Q: QueriesRunner,
{
    aggregate_manager: AM,
    event_store: ES,
    consumers: Vec<EC>,
    queries: Q,
}

impl<ES, Q, EC, AM> Cqrs<ES, Q, EC, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    EC: EventConsumersGroup,
    Q: QueriesRunner,
{
    pub fn new(aggregate_manager: AM, event_store: ES, consumers: Vec<EC>, queries: Q) -> Self {
        Self {
            aggregate_manager,
            event_store,
            consumers,
            queries,
        }
    }

    pub async fn execute<C>(
        &mut self,
        aggregate_id: Uuid,
        command: C,
    ) -> Result<(), CqrsError>
    where
        C: Command,
    {
        let mut aggregate = self.aggregate_manager.load::<C::Aggregate>(aggregate_id).await?;

        let events = command.handle(&aggregate).await?;

        self.event_store.save_events(aggregate_id, &events).await?;
        aggregate.apply_events(&events);

        for consumer in self.consumers.iter_mut() {
            for event in events.iter() {
                consumer.process(&event).await;
            }
        }

        self.aggregate_manager.store::<C::Aggregate>(&aggregate).await?;

        Ok(())
    }

    pub fn queries(&self) -> &Q {
        &self.queries
    }

    pub fn event_queries(&self) -> &ES {
        &self.event_store
    }
}
