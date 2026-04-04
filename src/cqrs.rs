use crate::{
    query::QueryRunner, Aggregate, AggregateManager, Command, CqrsError, Event, EventConsumers,
    EventStore, Uuid,
};

/// The `Cqrs` struct represents the main entry point of a CQRS application.
///
/// It orchestrates command execution: loading the aggregate, handling the command,
/// saving events (with optimistic concurrency), processing consumers, and storing the aggregate.
///
/// All methods take `&self`, so `Cqrs` can be wrapped in `Arc` for concurrent use.
pub struct Cqrs<ES, AM>
where
    AM: AggregateManager,
    ES: EventStore,
{
    aggregate_manager: AM,
    event_store: ES,
    consumers: EventConsumers,
}

impl<ES, AM> Cqrs<ES, AM>
where
    AM: AggregateManager,
    ES: EventStore,
{
    /// Creates a new Cqrs instance.
    pub fn new(aggregate_manager: AM, event_store: ES, consumers: EventConsumers) -> Self {
        Self {
            aggregate_manager,
            event_store,
            consumers,
        }
    }

    /// Executes a command on an aggregate.
    ///
    /// The flow:
    /// 1. Load aggregate from the aggregate manager
    /// 2. Execute the command, getting domain events
    /// 3. Wrap domain events into `Event` structs with version tracking
    /// 4. Save events to the event store (with optimistic concurrency check)
    /// 5. Apply events to the aggregate
    /// 6. Process events through consumers
    /// 7. Store the aggregate (e.g., snapshot)
    /// 8. Return the aggregate ID
    pub async fn execute<C>(&self, aggregate_id: Uuid, command: &C) -> Result<Uuid, CqrsError>
    where
        C: Command,
    {
        let mut aggregate = self
            .aggregate_manager
            .load::<C::Aggregate>(aggregate_id)
            .await?;

        let domain_events = command.handle(&aggregate).await?;

        let current_version = aggregate.version();
        let events: Vec<Event> = domain_events
            .into_iter()
            .enumerate()
            .map(|(i, payload)| Event::new(aggregate_id, payload, current_version + i as u64 + 1))
            .collect::<Result<Vec<_>, _>>()?;

        self.event_store
            .save_events(aggregate_id, &events, current_version)
            .await?;

        aggregate.apply_events(&events).await?;
        let new_version = events.last().map(|e| e.version).unwrap_or(current_version);
        aggregate.set_version(new_version);

        for event in events.iter() {
            self.consumers.process(event).await;
        }

        self.aggregate_manager
            .store::<C::Aggregate>(&aggregate)
            .await?;

        Ok(aggregate_id)
    }
}

/// Implements `QueryRunner` so you can call `cqrs.query(&q).await`.
impl<ES, AM> QueryRunner for Cqrs<ES, AM>
where
    AM: AggregateManager,
    ES: EventStore,
{
}
