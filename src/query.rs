use std::future::Future;

/// The `Query` trait represents a query that allows to retrieve information from read models.
pub trait Query: Send + Sync {
    /// The output type of the query.
    type Output;

    /// Executes the query and returns the result.
    fn apply(&self) -> impl Future<Output = Self::Output> + Send;
}

/// The `QueryRunner` trait can be implemented to provide a `query` method on your types.
pub trait QueryRunner {
    /// Executes a query and returns the result.
    fn query<Q>(&self, query: &Q) -> impl Future<Output = Q::Output> + Send
    where
        Q: Query,
    {
        async { query.apply().await }
    }
}
