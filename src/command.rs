use std::future::Future;

use crate::{Aggregate, CqrsError};

/// The `Command` trait defines the behavior of a command in a CQRS application.
///
/// A command is a request to change the state of an aggregate. Commands are handled by the
/// aggregate itself, and they return a list of domain events that describe the state changes.
/// The framework wraps these into `Event` structs with the aggregate ID and version.
///
/// ## Example
///
/// ```rust,ignore
/// use mini_cqrs_es::{Command, CqrsError};
///
/// struct CreateUser { name: String }
///
/// impl Command for CreateUser {
///     type Aggregate = UserAggregate;
///
///     async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<UserEvent>, CqrsError> {
///         Ok(vec![UserEvent::UserCreated { name: self.name.clone() }])
///     }
/// }
/// ```
pub trait Command: Send + Sync {
    /// The type of aggregate that this command handles.
    type Aggregate: Aggregate;

    /// Handles the command and returns a list of domain events.
    fn handle(
        &self,
        aggregate: &Self::Aggregate,
    ) -> impl Future<Output = Result<Vec<<Self::Aggregate as Aggregate>::Event>, CqrsError>> + Send;
}
