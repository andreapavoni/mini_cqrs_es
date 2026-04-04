use std::future::Future;

use crate::{
    query::QueryRunner, Aggregate, AggregateManager, Command, CqrsError, Event, EventConsumers,
    EventStore, Uuid,
};

/// The `Cqrs` trait represents the main entry point of a CQRS application.
///
/// It orchestrates command execution. Implementations determine how events
/// are dispatched after being saved (e.g., synchronous consumers, broadcast channels).
///
/// All methods take `&self`, so implementations can be wrapped in `Arc` for concurrent use.
pub trait Cqrs: QueryRunner + Send + Sync {
    /// Executes a command on an aggregate identified by `aggregate_id`.
    fn execute<C>(
        &self,
        aggregate_id: Uuid,
        command: &C,
    ) -> impl Future<Output = Result<Uuid, CqrsError>> + Send
    where
        C: Command;
}

/// A synchronous-consumer implementation of the [`Cqrs`] trait.
///
/// Events are dispatched through [`EventConsumers`], which processes each consumer
/// sequentially after events are saved.
///
/// The execution flow:
/// 1. Load aggregate from the aggregate manager
/// 2. Execute the command, getting domain events
/// 3. Wrap domain events into `Event` structs with version tracking
/// 4. Save events to the event store (with optimistic concurrency check)
/// 5. Apply events to the aggregate
/// 6. Process events through consumers
/// 7. Store the aggregate (e.g., snapshot)
/// 8. Return the aggregate ID
pub struct SimpleCqrs<ES, AM>
where
    AM: AggregateManager,
    ES: EventStore,
{
    aggregate_manager: AM,
    event_store: ES,
    consumers: EventConsumers,
}

impl<ES, AM> SimpleCqrs<ES, AM>
where
    AM: AggregateManager,
    ES: EventStore,
{
    /// Creates a new SimpleCqrs instance.
    pub fn new(aggregate_manager: AM, event_store: ES, consumers: EventConsumers) -> Self {
        Self {
            aggregate_manager,
            event_store,
            consumers,
        }
    }
}

impl<ES, AM> Cqrs for SimpleCqrs<ES, AM>
where
    AM: AggregateManager,
    ES: EventStore,
{
    async fn execute<C>(&self, aggregate_id: Uuid, command: &C) -> Result<Uuid, CqrsError>
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
impl<ES, AM> QueryRunner for SimpleCqrs<ES, AM>
where
    AM: AggregateManager,
    ES: EventStore,
{
}
