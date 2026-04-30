use std::future::Future;

use crate::{
    query::QueryRunner, Aggregate, AggregateManager, Command, CqrsError, EventConsumers,
    EventMetadata, EventStore, NewEvent,
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
        aggregate_id: &<C::Aggregate as Aggregate>::Id,
        command: &C,
    ) -> impl Future<Output = Result<<C::Aggregate as Aggregate>::Id, CqrsError>> + Send
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
/// 2. Execute the command, getting domain events or a semantic error
///    (`Domain` for business rules, `CommandInvariant` for application preconditions)
/// 3. Wrap domain events into `NewEvent` structs
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
    async fn execute<C>(
        &self,
        aggregate_id: &<C::Aggregate as Aggregate>::Id,
        command: &C,
    ) -> Result<<C::Aggregate as Aggregate>::Id, CqrsError>
    where
        C: Command,
    {
        let mut aggregate = self
            .aggregate_manager
            .load::<C::Aggregate>(aggregate_id)
            .await?;

        let domain_events = command.handle(&aggregate).await?;

        let current_version = aggregate.version();
        let new_events: Vec<NewEvent> = domain_events
            .into_iter()
            .map(|payload| NewEvent::from_payload(payload, EventMetadata::default()))
            .collect::<Result<Vec<_>, _>>()?;

        let events = self
            .event_store
            .save_events(
                std::any::type_name::<C::Aggregate>(),
                &aggregate_id.to_string(),
                &new_events,
                current_version,
            )
            .await?;

        aggregate.apply_events(&events).await?;
        let new_version = events.last().map(|e| e.version).unwrap_or(current_version);
        aggregate.set_version(new_version);

        for event in events.iter() {
            self.consumers.process(event).await?;
        }

        self.aggregate_manager
            .store::<C::Aggregate>(&aggregate)
            .await?;

        Ok(aggregate_id.clone())
    }
}

/// Implements `QueryRunner` so you can call `cqrs.query(&q).await`.
impl<ES, AM> QueryRunner for SimpleCqrs<ES, AM>
where
    AM: AggregateManager,
    ES: EventStore,
{
}
