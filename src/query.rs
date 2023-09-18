use async_trait::async_trait;

use crate::{Repository, CqrsError};

#[async_trait]
pub trait ModelReader: Send {
    type Repo: Repository;
    type Model: Send + Sync + Clone + 'static;

    async fn update(&mut self, data: Self::Model) -> Result<(), CqrsError>;
}


#[async_trait]
pub trait QueriesRunner {
    async fn run<A, B>(&self, query: A) -> B
    where
        A: Query + Query<Output = B> + Clone + Send + Sync,
    {
        query.apply().await
    }
}

#[async_trait]
pub trait Query: Send + Sync + Clone {
    type Output;

    async fn apply(&self) -> Self::Output;
}
