use async_trait::async_trait;

use crate::{CqrsError, Aggregate};

#[derive(Clone, Debug)]
pub struct AggregateSnapshot<T: Aggregate> {
    pub aggregate_id: T::Id,
    payload: serde_json::Value,
    pub version: u64,
}

impl<T: Aggregate> AggregateSnapshot<T> {
    pub fn new(payload: T, version: Option<u64>) -> Self {
        let version = version.unwrap_or(1);

        Self {
            aggregate_id: payload.aggregate_id(),
            payload: serde_json::to_value(payload).unwrap(),
            version,
        }
    }

    pub fn get_payload(&self) -> T {
        serde_json::from_value(self.payload.clone()).unwrap()
    }
}

#[async_trait]
pub trait SnapshotStore<T: Aggregate> {
    async fn save_snapshot(
        &mut self,
        aggregate: AggregateSnapshot<T>,
    ) -> Result<(), CqrsError>;

    async fn load_snapshot(
        &self,
        aggregate_id: <T as Aggregate>::Id,
    ) -> Result<AggregateSnapshot<T>, CqrsError>;
}
