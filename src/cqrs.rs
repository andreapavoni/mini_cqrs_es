use std::marker::PhantomData;

use crate::{Aggregate, CqrsError, Dispatcher, EventStore, QueriesRunner};

#[derive(Clone)]
pub struct Cqrs<D, A, ES, Q>
where
    D: Dispatcher<A, ES>,
    A: Aggregate,
    ES: EventStore<AggregateId = A::Id>,
    Q: QueriesRunner,
{
    dispatcher: D,
    queries: Q,
    marker: PhantomData<(A, ES)>,
}

impl<D, A, ES, Q> Cqrs<D, A, ES, Q>
where
    D: Dispatcher<A, ES>,
    A: Aggregate,
    ES: EventStore<AggregateId = A::Id>,
    Q: QueriesRunner,
{
    pub fn new(dispatcher: D, queries: Q) -> Self {
        Self {
            dispatcher,
            marker: PhantomData,
            queries,
        }
    }

    pub async fn execute(
        &mut self,
        aggregate_id: A::Id,
        command: A::Command,
    ) -> Result<A::Id, CqrsError> {
        match self.dispatcher.execute(aggregate_id.clone(), command).await {
            Ok(_) => Ok(aggregate_id),
            Err(err) => Err(err),
        }
    }

    pub fn queries(&self) -> &Q {
        &self.queries
    }
}
