use async_trait::async_trait;

use crate::{CqrsError, Repository};

/// The `ModelReader` trait defines the behavior of a read model, it' responsible for updating read models with data extracted from events.
///
/// This trait must be implemented by all model readers in your application.
#[async_trait]
pub trait ModelReader: Send {
    /// The associated repository type for this model reader.
    type Repo: Repository;

    /// The model type that this reader reads and updates.
    type Model: Send + Sync + Clone + 'static;

    /// Updates the read model with the provided data.
    async fn update(&mut self, data: Self::Model) -> Result<(), CqrsError>;
}

/// The `QueriesRunner` trait defines the behavior for executing queries on read models.
///
/// When `impl`ed on a type, it allows you to execute queries and return results. It's used by
/// `Cqrs` to handle queries, but it can also be useful for consumers or other parts of your
/// application.
#[async_trait]
pub trait QueriesRunner {
    /// Executes a query and returns the result.
    async fn query<A, B>(&self, query: &A) -> B
    where
        A: Query + Query<Output = B> + Clone + Send + Sync,
    {
        query.apply().await
    }
}

/// The `Query` trait represents a query that allows to retrieve information from read models.
///
/// This trait must be implemented by all queries in your application.
#[async_trait]
pub trait Query: Send + Sync + Clone {
    /// The output type of the query.
    type Output;

    /// Executes the query and returns the result.
    async fn apply(&self) -> Self::Output;
}
