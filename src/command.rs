use crate::{Aggregate, Event, Result};
use async_trait::async_trait;

/// The `Command` trait defines the behavior of a command in a Command-Query Responsibility Segregation (CQRS) application.
///
/// A command is a request to change the state of an aggregate. Commands are handled by the aggregate itself, and they return
/// a list of events that describe the state changes. These events are used to update the aggregate's state and are subsequently
/// stored.
///
/// ## Implementing the `Command` Trait
///
/// To create a command in your application, you need to implement this trait for each specific command type. You should specify
/// the type of the aggregate that the command handles in the associated `type` and implement the `handle` method to define how the
/// command is processed.
///
/// ## Example
///
/// Here's a basic example of implementing the `Command` trait for a specific command type:
///
/// ```rust
/// use mini_cqrs_es::{Command, Aggregate, CqrsError, Event};
///
/// struct MyCommand;
///
/// impl Command for MyCommand {
///     type Aggregate = MyAggregate;
///
///     async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<Event>, CqrsError> {
///         // Implement your command logic here.
///         unimplemented!()
///     }
/// }
/// ```
///

#[async_trait]
// Define Ctx as a generic parameter with a default
pub trait Command<Ctx = ()>: Send + Sync
where
    Ctx: Send + Sync + 'static,
{
    type Aggregate: Aggregate + Send + Sync; // Ensure aggregate is Send+Sync

    /// Handles the command using the aggregate's current state and external context.
    /// Returns a list of events generated or a CqrsError.
    async fn handle(&self, aggregate: &Self::Aggregate, ctx: &Ctx) -> Result<Vec<Event>>; // Use Ctx generic param
}
