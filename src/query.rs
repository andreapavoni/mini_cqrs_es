use async_trait::async_trait;

use crate::{CqrsError, Repository};

/// A trait that defines the behavior of a model reader.
///
/// A model reader is responsible for updating read models with data extracted from events.
///
#[async_trait]
pub trait ModelReader: Send {
    /// The associated repository type for this model reader.
    type Repo: Repository;

    /// The model type that this reader reads and updates.
    type Model: Send + Sync + Clone + 'static;

    /// Updates the read model with the provided data.
    async fn update(&mut self, data: Self::Model) -> Result<(), CqrsError>;
}

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

/// A trait representing a query on read models.
///
/// Queries allow you to retrieve information from read models.
///
#[async_trait]
pub trait Query: Send + Sync + Clone {
    /// The output type of the query.
    type Output;

    /// Executes the query and returns the result.
    async fn apply(&self) -> Self::Output;
}
