#![cfg(any(feature = "turso", feature = "postgresql"))]

use mini_cqrs_es::*;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Counter {
    value: u32,
}
#[derive(Serialize, Deserialize)]
struct Added(u32);
impl EventPayload for Added {
    fn event_type(&self) -> &'static str {
        "counter.added"
    }
}
impl Aggregate for Counter {
    type Id = String;
    type Event = Added;
    fn aggregate_type() -> &'static str {
        "counter"
    }
    async fn apply(&mut self, event: &Added) {
        self.value += event.0;
    }
}
struct Add;
impl Command for Add {
    type Aggregate = Counter;
    async fn handle(&self, _: &Counter) -> Result<Vec<Added>, CqrsError> {
        Ok(vec![Added(3)])
    }
}
#[derive(Debug, toasty::Model)]
struct CounterView {
    #[key]
    id: String,
    value: u32,
}
struct Totals;
impl Projection for Totals {
    type Aggregate = Counter;
    fn name() -> &'static str {
        "totals"
    }
    async fn apply(
        &self,
        event: &StoredEvent,
        payload: Added,
        executor: &mut dyn toasty::Executor,
    ) -> Result<(), CqrsError> {
        CounterView::upsert_by_id(&event.aggregate_id)
            .value(payload.0)
            .exec(executor)
            .await
            .map_err(|error| CqrsError::storage("write view", error))?;
        Ok(())
    }
}

#[path = "support/database.rs"]
mod database;

#[tokio::test]
async fn setup_is_backend_independent_explicit_and_support_models_are_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let event_url = database::fresh_url(&directory.path().join("events.db"), false).await;
    let reads_url = database::fresh_url(&directory.path().join("reads.db"), true).await;
    let events = EventLog::connect(&event_url).await.unwrap();
    let read_models = || ReadModels::builder().register::<CounterView>();
    let reads = read_models().connect(&reads_url).await.unwrap();
    assert!(events.stream_version("counter", "one").await.is_err());
    events.create_schema().await.unwrap();
    reads.create_schema().await.unwrap();
    let projector = reads.projector(Totals);
    let engine = SimpleCqrs::new(events.clone())
        .with_snapshots(events.snapshot_cache())
        .with_consumer(projector.clone());
    let id = "one".to_owned();
    engine
        .execute(&id, &Add, EventMetadata::default())
        .await
        .unwrap();
    let history =
        serde_json::to_value(events.load_events("counter", &id).await.unwrap().0).unwrap();
    let reopened = EventLog::connect(&event_url).await.unwrap();
    let reopened_reads = read_models().connect(&reads_url).await.unwrap();
    assert_eq!(
        reopened_reads
            .projector(Totals)
            .last_applied(&id)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        CounterView::get_by_id(&mut reopened_reads.database(), &id)
            .await
            .unwrap()
            .value,
        3
    );
    assert_eq!(
        reopened
            .snapshot_cache()
            .load_snapshot::<Counter>(&id)
            .await
            .unwrap()
            .unwrap()
            .version,
        1
    );
    assert_eq!(
        history,
        serde_json::to_value(reopened.load_events("counter", &id).await.unwrap().0).unwrap()
    );
    assert!(
        toasty::sql::query("SELECT * FROM cqrs_events")
            .exec(&mut reads.database())
            .await
            .is_err()
    );
    assert!(
        toasty::sql::query("SELECT * FROM cqrs_snapshots")
            .exec(&mut reads.database())
            .await
            .is_err()
    );
}
