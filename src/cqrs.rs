use crate::{
    query::QueriesRunner, Aggregate, AggregateManager, Command, EventConsumersGroup, EventStore,
    Uuid,
};
use anyhow::Error;

/// The `Cqrs` struct represents the main entry point of a Command-Query Responsibility Segregation (CQRS) application.
///
/// CQRS is an architectural pattern that separates the reading and writing sides of an application to achieve better performance, scalability, and maintainability. The `Cqrs` type acts as a core component, providing a structure for handling commands, queries, and event processing.
///
/// ## Example
///
/// Here's an example of how to create and use a `Cqrs` instance in a CQRS application:
///
/// ```rust
/// use mini_cqrs_es::{Cqrs, AggregateManager, EventStore, EventConsumersGroup, Command, Uuid};
/// use anyhow::Error;
///
/// // Define custom aggregate manager, event store, and event consumers.
/// struct MyAggregateManager;
/// struct MyEventStore;
/// struct MyEventConsumers;
///
/// // Implement the necessary traits for these components.
/// impl AggregateManager for MyAggregateManager {
///     // Implement the methods of AggregateManager
/// }
/// impl EventStore for MyEventStore {
///     // Implement the methods of EventStore
/// }
/// impl EventConsumersGroup for MyEventConsumers {
///     // Implement the methods of EventConsumersGroup
/// }
///
/// // Define a custom command type.
/// struct MyCommand;
///
/// // Implement the Command trait for the custom command.
/// #[async_trait::async_trait]
/// impl Command for MyCommand {
///     type Aggregate = MyAggregate;  // Replace with your own aggregate type
///
///     async fn handle(&self, aggregate: &MyAggregate) -> Result<Vec<Event>, Error> {
///         // Implement command handling logic
///         unimplemented!()
///     }
/// }
///
/// // Create a Cqrs instance.
/// let aggregate_manager = MyAggregateManager;
/// let event_store = MyEventStore;
/// let consumers = MyEventConsumers;
/// let mut cqrs = Cqrs::new(aggregate_manager, event_store, consumers);
///
/// // Execute a command using the Cqrs instance.
/// let aggregate_id = Uuid::new_v4();
/// let command = MyCommand;
/// match cqrs.execute(aggregate_id, &command).await {
///     Ok(_aggregate_id) => println!("Command executed successfully!"),
///     Err(err) => eprintln!("Error: {:?}", err),
/// }
/// ```
pub struct Cqrs<ES, EC, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    EC: EventConsumersGroup,
{
    /// The aggregate manager.
    aggregate_manager: AM,

    /// The event store.
    event_store: ES,

    /// The event consumers.
    consumers: EC,
}

impl<ES, EC, AM> Cqrs<ES, EC, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    EC: EventConsumersGroup,
{
    /// Creates a new Cqrs instance.
    pub fn new(aggregate_manager: AM, event_store: ES, consumers: EC) -> Self {
        Self {
            aggregate_manager,
            event_store,
            consumers,
        }
    }

    /// Executes a command on an aggregate.
    pub async fn execute<C>(&mut self, aggregate_id: Uuid, command: &C) -> Result<Uuid, Error>
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

        for event in events.iter() {
            self.consumers.process(event).await;
        }

        self.aggregate_manager
            .store::<C::Aggregate>(&aggregate)
            .await?;

        Ok(aggregate_id)
    }
}

/// Defines `query` function to run `Query` from `Cqrs`.
impl<ES, EC, AM> QueriesRunner for Cqrs<ES, EC, AM>
where
    AM: AggregateManager,
    ES: EventStore,
    EC: EventConsumersGroup,
{
}
