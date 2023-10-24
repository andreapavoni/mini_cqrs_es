use crate::{
    query::QueriesRunner, Aggregate, AggregateManager, Command, CqrsError, EventConsumersGroup,
    EventStore,
};
use uuid::Uuid;

pub struct Cqrs<ES, EC, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    EC: EventConsumersGroup,
{
    aggregate_manager: AM,
    event_store: ES,
    consumers: Vec<EC>,
}

impl<ES, EC, AM> Cqrs<ES, EC, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    EC: EventConsumersGroup,
{
    pub fn new(aggregate_manager: AM, event_store: ES, consumers: Vec<EC>) -> Self {
        Self {
            aggregate_manager,
            event_store,
            consumers,
        }
    }

    pub async fn execute<C>(&mut self, aggregate_id: Uuid, command: C) -> Result<(), CqrsError>
    where
        C: Command,
    {
        let mut aggregate = self
            .aggregate_manager
            .load::<C::Aggregate>(aggregate_id)
            .await?;

        let events = command.handle(&aggregate).await?;

        self.event_store.save_events(aggregate_id, &events).await?;
        aggregate.apply_events(&events).await;

        for consumer in self.consumers.iter_mut() {
            for event in events.iter() {
                consumer.process(&event).await;
            }
        }

        self.aggregate_manager
            .store::<C::Aggregate>(&aggregate)
            .await?;

        Ok(())
    }
}

impl<ES, EC, AM> QueriesRunner for Cqrs<ES, EC, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    EC: EventConsumersGroup,
{
}
