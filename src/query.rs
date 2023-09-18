/// A trait representing a model reader for read models.
///
/// Model readers allow you to perform queries on read models and update them as needed.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::{CqrsError, ModelReader, Repository};
///
/// struct YourModelReader {
///     repo: YourRepository,
/// }
///
/// impl ModelReader for YourModelReader {
///     type Repo = YourRepository;
///     type Model = YourModel;
///
///     async fn update(&mut self, data: YourModel) -> Result<(), CqrsError> {
///         // Implement the update logic here.
///     }
/// }
/// ```

use async_trait::async_trait;

use crate::{Repository, CqrsError};

#[async_trait]
pub trait ModelReader: Send {
    /// The associated repository type for this model reader.
    type Repo: Repository;
    
    /// The model type that this reader reads and updates.
    type Model: Send + Sync + Clone + 'static;

    /// Updates the read model with the provided data.
    ///
    /// # Parameters
    ///
    /// - `data`: The updated data for the read model.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an error if the update fails.
    async fn update(&mut self, data: Self::Model) -> Result<(), CqrsError>;
}

/// A trait representing a runner for queries on read models.
///
/// Queries runners allow you to execute queries on read models and retrieve results.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::{QueriesRunner, Query};
///
/// struct YourQueriesRunner;
///
/// impl QueriesRunner for YourQueriesRunner {}
/// ```
#[async_trait]
pub trait QueriesRunner {
    /// Executes a query and returns the result.
    ///
    /// # Parameters
    ///
    /// - `query`: The query to execute.
    ///
    /// # Returns
    ///
    /// The result of the query execution.
    async fn run<A, B>(&self, query: A) -> B
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
/// # Example
///
/// ```rust
/// use mini_cqrs::{Query, QueriesRunner};
///
/// struct YourQuery;
///
/// impl Query for YourQuery {
///     type Output = YourQueryResult;
///
///     async fn apply(&self) -> Self::Output {
///         // Implement the query logic here.
///     }
/// }
/// ```
#[async_trait]
pub trait Query: Send + Sync + Clone {
    /// The output type of the query.
    type Output;

    /// Executes the query and returns the result.
    ///
    /// # Returns
    ///
    /// The result of the query execution.
    async fn apply(&self) -> Self::Output;
}
