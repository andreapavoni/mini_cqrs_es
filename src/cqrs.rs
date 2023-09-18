use std::marker::PhantomData;

use crate::{Aggregate, CqrsError, Dispatcher, EventStore, QueriesRunner};

/// A CQRS (Command Query Responsibility Segregation) engine for managing aggregates and executing queries.
///
/// The `Cqrs` struct serves as the central component for managing the Command and Query aspects of the CQRS pattern.
/// It allows you to execute commands that modify aggregates and perform queries to retrieve read model data.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::{Cqrs, YourDispatcher, YourQueriesRunner};
///
/// // Create a dispatcher and queries runner.
/// let dispatcher = YourDispatcher::new();
/// let queries_runner = YourQueriesRunner::new();
///
/// // Create a Cqrs instance.
/// let mut cqrs = Cqrs::new(dispatcher, queries_runner);
///
/// // Execute a command.
/// let aggregate_id = "example_aggregate_id";
/// let command = YourCommand::new(/* command parameters */);
/// let result = cqrs.execute(aggregate_id.to_string(), command).await;
///
/// // Perform a query.
/// let query = YourQuery::new(/* query parameters */);
/// let query_result = cqrs.queries().run(query).await;
/// ```
///
/// The `Cqrs` struct is parameterized by four generic types:
///
/// - `D`: The dispatcher responsible for handling commands and events.
/// - `A`: The aggregate type associated with the CQRS engine.
/// - `ES`: The event store used to store and retrieve events for aggregates.
/// - `Q`: The queries runner responsible for executing queries.
///
/// The dispatcher is responsible for managing commands and events, the aggregate represents the business entity being modified,
/// the event store stores the events, and the queries runner executes read model queries.
#[derive(Clone)]
pub struct Cqrs<D, A, ES, Q>
where
    D: Dispatcher<A, ES>,
    A: Aggregate,
    ES: EventStore<AggregateId = A::Id>,
    Q: QueriesRunner,
{
    dispatcher: D,
    queries: Q,
    marker: PhantomData<(A, ES)>,
}

impl<D, A, ES, Q> Cqrs<D, A, ES, Q>
where
    D: Dispatcher<A, ES>,
    A: Aggregate,
    ES: EventStore<AggregateId = A::Id>,
    Q: QueriesRunner,
{
    /// Creates a new `Cqrs` instance with the provided dispatcher and queries runner.
    ///
    /// # Parameters
    ///
    /// - `dispatcher`: An instance of the dispatcher responsible for handling commands and events.
    /// - `queries`: An instance of the queries runner responsible for executing queries.
    ///
    /// # Returns
    ///
    /// A new `Cqrs` instance.
    pub fn new(dispatcher: D, queries: Q) -> Self {
        Self {
            dispatcher,
            marker: PhantomData,
            queries,
        }
    }

    /// Executes a command to modify an aggregate and returns the aggregate's identifier.
    ///
    /// This method takes a command and triggers its execution through the associated dispatcher.
    ///
    /// # Parameters
    ///
    /// - `aggregate_id`: The identifier of the aggregate to which the command applies.
    /// - `command`: The command to execute.
    ///
    /// # Returns
    ///
    /// - `Ok(aggregate_id)`: If the command execution is successful, returns the aggregate's identifier.
    /// - `Err(err)`: If an error occurs during command execution, returns a `CqrsError` with details about the error.
    pub async fn execute(
        &mut self,
        aggregate_id: A::Id,
        command: A::Command,
    ) -> Result<A::Id, CqrsError> {
        match self.dispatcher.execute(aggregate_id.clone(), command).await {
            Ok(_) => Ok(aggregate_id),
            Err(err) => Err(err),
        }
    }

    /// Returns a reference to the queries runner associated with this `Cqrs` instance.
    ///
    /// You can use the queries runner to perform read model queries.
    ///
    /// # Returns
    ///
    /// A reference to the queries runner.
    pub fn queries(&self) -> &Q {
        &self.queries
    }
}
