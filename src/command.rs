use async_trait::async_trait;

use crate::{Aggregate, CqrsError, Event};

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
pub trait Command {
    /// The type of aggregate that this command handles.
    type Aggregate: Aggregate;

    /// Handles the command and returns a list of events.
    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<Event>, CqrsError>;
}
