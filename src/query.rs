use async_trait::async_trait;

use crate::{Repository, CqrsError};

#[async_trait]
pub trait Query: Send + Sync + Clone {
    type Output: Clone;
    type Repo: Repository;

    async fn run(&self, repo: Self::Repo) -> Result<Self::Output, CqrsError>;
}

#[async_trait]
pub trait ModelReader: Send {
    type Repo: Repository;
    type Query: Send + Sync + Clone + 'static;
    type Output: Send + Sync + 'static;

    async fn query(&self, query: Self::Query) -> Result<Self::Output, CqrsError>;
    async fn update(&mut self, data: Self::Output) -> Result<(), CqrsError>;
}
