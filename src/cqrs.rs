use std::fmt::Debug;
use tokio::sync::mpsc; // For the sender type

use crate::{
    query::QueriesRunner, Aggregate, AggregateManager, Command, CqrsError, EventConsumersGroup,
    EventStore, Result, Uuid,
};

// use crate::{
//     query::QueriesRunner, Aggregate, AggregateManager, Command, CqrsError, EventConsumersGroup,
//     EventStore, Result, Uuid,
// };

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

#[derive(Clone)]
pub struct Cqrs<ES, EC, AM, Ctx, M>
where
    AM: AggregateManager,
    ES: EventStore,
    EC: EventConsumersGroup<M>, // Group processes events returning Vec<M>
    Ctx: Send + Sync + Clone + 'static,
    M: Send + Debug + 'static, // Command message type for the bus/channel
{
    aggregate_manager: AM,
    event_store: ES,
    consumers: EC,
    context: Ctx,
    // Holds the sender for the external command processing loop
    command_sender: mpsc::Sender<M>,
}

impl<ES, EC, AM, Ctx, M> Cqrs<ES, EC, AM, Ctx, M>
where
    AM: AggregateManager + Send + Sync + Clone,
    ES: EventStore + Send + Sync + Clone,
    EC: EventConsumersGroup<M> + Send + Sync + Clone, // Bounds for Group<M>
    Ctx: Send + Sync + Clone + 'static,
    M: Send + Debug + 'static,
{
    /// Creates a new Cqrs instance with context and command sender.
    pub fn new(
        aggregate_manager: AM,
        event_store: ES,
        consumers: EC,
        context: Ctx,
        command_sender: mpsc::Sender<M>, // Accept command sender
    ) -> Self {
        Self {
            aggregate_manager,
            event_store,
            consumers,
            context,
            command_sender, // Store sender
        }
    }

    /// Executes a command on an aggregate.
    pub async fn execute<C>(&mut self, aggregate_id: Uuid, command: &C) -> Result<Uuid>
    where
        C: Command<Ctx> + Send + Sync,
        C::Aggregate: Aggregate + Send + Sync + 'static,
        // No direct constraint on Aggregate::Event needed here anymore
    {
        // 1. Load aggregate & get current version
        let mut aggregate = self
            .aggregate_manager
            .load::<C::Aggregate>(aggregate_id)
            .await?;
        let current_version = self
            .event_store
            .load_events(aggregate_id)
            .await
            .map_err(|e| CqrsError::StoreOperation {
                aggregate_id,
                source: e,
            })?
            .last()
            .map_or(0, |e| e.version);

        // 2. Handle command using context
        let new_events = command.handle(&aggregate, &self.context).await?;

        // 3. Assign correct versions
        let mut versioned_events = Vec::with_capacity(new_events.len());
        let mut next_version = current_version + 1;
        for mut event in new_events {
            if event.aggregate_id != aggregate_id {
                return Err(CqrsError::CommandValidation {
                    aggregate_id,
                    reason: format!(
                        "Event aggregate ID {} does not match target aggregate ID {}",
                        event.aggregate_id, aggregate_id
                    ),
                });
            }
            event.version = next_version;
            versioned_events.push(event);
            next_version += 1;
        }

        // 4. Save events
        if !versioned_events.is_empty() {
            self.event_store
                .save_events(aggregate_id, &versioned_events)
                .await
                .map_err(|e| CqrsError::StoreOperation {
                    aggregate_id,
                    source: e,
                })?;

            // 5. Apply events locally
            aggregate.apply_events(&versioned_events).await;

            // 6. Process events via consumers and collect commands to dispatch
            let mut commands_to_dispatch = Vec::new();
            for event in versioned_events.iter() {
                // consumers.process now returns Result<Vec<M>>
                // TODO: Pass context if consumer process signature changes
                let mut dispatched_by_consumers = self.consumers.process(event).await?;
                commands_to_dispatch.append(&mut dispatched_by_consumers);
            }

            // 7. Dispatch collected commands
            for cmd_msg in commands_to_dispatch {
                self.command_sender.send(cmd_msg).await.map_err(|e| {
                    CqrsError::CommandDispatch(format!("Failed to send command via bus: {}", e))
                })?;
            }

            // 8. Optional: Store aggregate snapshot
            self.aggregate_manager
                .store::<C::Aggregate>(&aggregate)
                .await?;
        }

        Ok(aggregate_id)
    }

    // run_command_processor method is NOT part of Cqrs struct anymore.
    // The application's main loop runs the receiver.
}

// Implement QueriesRunner trait (no change needed)
impl<ES, EC, AM, Ctx, M> QueriesRunner for Cqrs<ES, EC, AM, Ctx, M>
where
    AM: AggregateManager + Send + Sync + Clone,
    ES: EventStore + Send + Sync + Clone,
    EC: EventConsumersGroup<M> + Send + Sync + Clone,
    Ctx: Send + Sync + Clone + 'static,
    M: Send + Debug + 'static,
{
    /* Uses default */
}
