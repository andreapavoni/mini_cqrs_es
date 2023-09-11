use std::marker::PhantomData;

use crate::{Aggregate, CqrsError, Dispatcher, EventStore, ModelReader};

pub struct Cqrs<D, A, ES>
where
    D: Dispatcher<A, ES>,
    A: Aggregate,
    ES: EventStore<AggregateId = A::Id>,
{
    dispatcher: D,
    marker: PhantomData<(A, ES)>,
}

impl<D, A, ES> Cqrs<D, A, ES>
where
    D: Dispatcher<A, ES>,
    A: Aggregate,
    ES: EventStore<AggregateId = A::Id>,
{
    pub fn new(dispatcher: D) -> Self {
        Self {
            dispatcher,
            marker: PhantomData,
        }
    }

    pub async fn execute(
        &mut self,
        aggregate_id: A::Id,
        command: A::Command,
    ) -> Result<(), CqrsError> {
        match self.dispatcher.execute(aggregate_id, command).await {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        }
    }

    pub async fn query<MR: ModelReader>(
        &self,
        reader: MR,
        query: MR::Query,
    ) -> Result<<MR as ModelReader>::Output, CqrsError> {
        reader.query(query).await
    }
}
