use std::{fmt::Debug, marker::PhantomData};

use async_trait::async_trait;

use crate::{Aggregate, CqrsError, Dispatcher, EventConsumer, EventStore, SnapshotStore, AggregateSnapshot};

pub struct SnapshotDispatcher<A, ES, SS>
where
    A: Aggregate,
    ES: EventStore,
    SS: SnapshotStore<A>,
{
    event_store: ES,
    snapshot_store: SS,
    event_consumers: Vec<Box<dyn EventConsumer>>,
    marker: PhantomData<A>,
}

impl<A, ES, SS> SnapshotDispatcher<A, ES, SS>
where
    A: Aggregate,
    ES: EventStore,
    SS: SnapshotStore<A>,
{
    pub fn new(
        event_store: ES,
        snapshot_store: SS,
        event_consumers: Vec<Box<dyn EventConsumer>>,
    ) -> Self {
        Self {
            event_store,
            snapshot_store,
            event_consumers,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<A, ES, SS> Dispatcher<A, ES> for SnapshotDispatcher<A, ES, SS>
where
    A: Aggregate + Clone,
    ES: EventStore<AggregateId = A::Id> + Send + Sync,
    SS: SnapshotStore<A> + Send + Sync,
    A::Command: Send + Sync,
    A::Event: Debug + Send + Sync,
{
    async fn execute(&mut self, aggregate_id: A::Id, command: A::Command) -> Result<A, CqrsError> {
        let mut aggregate = self.load_aggregate(&aggregate_id).await;

        aggregate.set_aggregate_id(aggregate_id.clone());

        let events = aggregate.handle(command).await?;
        self.event_store.save_events(aggregate_id, &events).await?;

        aggregate.apply_events(&events);

        self.snapshot_store.save_snapshot(AggregateSnapshot::new(aggregate.clone(), None)).await?;

        for consumer in self.event_consumers.iter_mut() {
            for event in &events {
                consumer.process(event).await;
            }
        }

        Ok(aggregate)
    }

    async fn load_aggregate(&self, aggregate_id: &A::Id) -> A {
        if let Ok(snapshot) = self
            .snapshot_store
            .load_snapshot(aggregate_id.clone())
            .await
        {
            snapshot.get_payload()
        } else {
            A::default()
        }
    }
}
