use async_trait::async_trait;

use crate::{Aggregate, CqrsError, Event};

/// A trait that defines the behavior of a command.
///
/// A command is a request to change the state of an aggregate. Commands are handled by the aggregate itself and return a list of events.
///
/// This trait must be implemented by all commands in your application.
#[async_trait]
pub trait Command {
    /// The type of aggregate that this command handles.
    type Aggregate: Aggregate;

    /// Handles the command and returns a list of events.
    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<Event>, CqrsError>;
}
