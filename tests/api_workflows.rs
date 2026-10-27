#![cfg(any(feature = "turso", feature = "postgresql"))]
#[path = "support/database.rs"]
mod database;

use mini_cqrs_es::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{num::NonZeroUsize, sync::Arc};
use toasty::schema::Model;

// No Hash/FromStr on the ID; no Clone/Debug on state or Clone/Display on events.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Id(String);
impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Default, Serialize, Deserialize)]
struct Balance {
    total: u64,
}
#[derive(Serialize, Deserialize)]
struct Credited {
    value: u64,
}
impl EventPayload for Credited {
    const SCHEMA_VERSION: u32 = 2;
    fn event_type(&self) -> &'static str {
        "credited"
    }
    fn decode(kind: &str, revision: u32, payload: Value) -> Result<Self, CqrsError> {
        let payload = match revision {
            1 => {
                json!({"value": payload.get("old_value").ok_or_else(|| CqrsError::EventFormat("missing old_value".into()))?})
            }
            2 => payload,
            _ => return Err(CqrsError::EventFormat("unknown revision".into())),
        };
        Self::decode_current(kind, payload)
    }
}
impl Aggregate for Balance {
    type Id = Id;
    type Event = Credited;
    fn aggregate_type() -> &'static str {
        "test.balance"
    }
    async fn apply(&mut self, event: &Credited) {
        self.total += event.value;
    }
}
struct Add(u64);
impl Command for Add {
    type Aggregate = Balance;
    async fn handle(&self, _: &Balance) -> Result<Vec<Credited>, CqrsError> {
        Ok(vec![Credited { value: self.0 }])
    }
}
// Deliberately non-Clone custom store, shared by the constructor and Arc forwarding.
struct CustomStore(EventLog);
impl EventStore for CustomStore {
    async fn save_events(
        &self,
        kind: &str,
        id: &str,
        events: &[NewEvent],
        expected: u64,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        self.0.save_events(kind, id, events, expected).await
    }
    async fn stream_version(&self, kind: &str, id: &str) -> Result<u64, CqrsError> {
        self.0.stream_version(kind, id).await
    }
    async fn load_events_after(
        &self,
        kind: &str,
        id: &str,
        after: u64,
        through: u64,
        limit: NonZeroUsize,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        self.0
            .load_events_after(kind, id, after, through, limit)
            .await
    }
    async fn list_streams_after(
        &self,
        after: Option<&StreamKey>,
        limit: NonZeroUsize,
    ) -> Result<Vec<StreamKey>, CqrsError> {
        self.0.list_streams_after(after, limit).await
    }
}
struct CustomSnapshots(SnapshotCache);
impl SnapshotStore for CustomSnapshots {
    async fn save_snapshot<A: Aggregate>(
        &self,
        snapshot: AggregateSnapshot<A>,
    ) -> Result<(), CqrsError> {
        self.0.save_snapshot(snapshot).await
    }
    async fn load_snapshot<A: Aggregate>(
        &self,
        id: &A::Id,
    ) -> Result<Option<AggregateSnapshot<A>>, CqrsError> {
        self.0.load_snapshot(id).await
    }
}
#[derive(Debug, toasty::Model)]
struct TotalRow {
    #[key]
    id: String,
    value: u64,
}
struct Totals<const N: u8> {
    fail: bool,
}
impl<const N: u8> Projection for Totals<N> {
    type Aggregate = Balance;
    fn name() -> &'static str {
        if N == 0 { "first" } else { "second" }
    }
    async fn apply(
        &self,
        event: &StoredEvent,
        payload: Credited,
        executor: &mut dyn toasty::Executor,
    ) -> Result<(), CqrsError> {
        if self.fail && event.version == 2 {
            return Err(CqrsError::domain("injected failure"));
        }
        let id = format!("{N}-{}", event.aggregate_id);
        let previous = TotalRow::filter_by_id(&id)
            .first()
            .exec(&mut *executor)
            .await
            .map_err(|e| CqrsError::storage("load total", e))?
            .map_or(0, |row| row.value);
        TotalRow::upsert_by_id(id)
            .value(previous + payload.value)
            .exec(executor)
            .await
            .map_err(|e| CqrsError::storage("save total", e))?;
        Ok(())
    }
}
struct Fixture {
    events: EventLog,
    reads: toasty::Db,
    url: String,
    _temp: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let url = database::fresh_url(&temp.path().join("events.db"), false).await;
        let events = EventLog::connect(&url).await.unwrap();
        events.database().push_schema().await.unwrap();
        let mut models = toasty::ModelSet::new();
        ProjectionCheckpoint::register(&mut models);
        TotalRow::register(&mut models);
        let reads = toasty::Db::builder()
            .models(models)
            .connect(&database::fresh_url(&temp.path().join("reads.db"), true).await)
            .await
            .unwrap();
        reads.push_schema().await.unwrap();
        Self {
            events,
            reads,
            url,
            _temp: temp,
        }
    }
    async fn seed(&self) {
        let old = NewEvent {
            event_type: "credited".into(),
            schema_version: 1,
            payload: json!({"old_value":3}),
            metadata: EventMetadata::default(),
            timestamp: chrono::Utc::now(),
        };
        let new = NewEvent::from_payload(Credited { value: 7 }, EventMetadata::default()).unwrap();
        self.events
            .save_events(Balance::aggregate_type(), "one", &[old, new], 0)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn shared_non_clone_stores_and_builder_order_preserve_consumers_and_snapshot_recovery() {
    let f = Fixture::new().await;
    let events = Arc::new(CustomStore(f.events.clone()));
    let snapshots = Arc::new(CustomSnapshots(f.events.snapshot_cache()));
    let projection = Projector::new(f.reads.clone(), Totals::<0> { fail: false });
    let id = Id("one".into());
    let first = SimpleCqrs::new(events.clone())
        .with_consumer(projection)
        .with_consumers(
            EventConsumers::new()
                .with(Projector::new(f.reads.clone(), Totals::<1> { fail: false })),
        )
        .with_snapshots(snapshots.clone());
    first
        .execute(&id, &Add(3), EventMetadata::default())
        .await
        .unwrap();
    assert_eq!(
        Projector::new(f.reads.clone(), Totals::<1> { fail: false })
            .last_applied(&id)
            .await
            .unwrap(),
        1
    );
    let second = SimpleCqrs::new(events.clone())
        .with_snapshots(snapshots.clone())
        .with_consumer(Projector::new(f.reads.clone(), Totals::<0> { fail: false }));
    let receipt = second
        .execute(&id, &Add(7), EventMetadata::default())
        .await
        .unwrap();
    assert_eq!(receipt.version, 2);
    let loaded: LoadedAggregate<Balance> =
        SnapshotAggregateManager::new(events.clone(), snapshots.clone())
            .load(&id)
            .await
            .unwrap();
    assert_eq!(loaded.state().total, 10);
    assert_eq!(
        snapshots
            .load_snapshot::<Balance>(&id)
            .await
            .unwrap()
            .unwrap()
            .version,
        2
    );
    assert_eq!(
        Projector::new(f.reads, Totals::<0> { fail: false })
            .last_applied(&id)
            .await
            .unwrap(),
        2
    );
    assert_eq!(events.list_streams().await.unwrap().len(), 1);
    assert_eq!(
        events
            .load_events(Balance::aggregate_type(), "one")
            .await
            .unwrap()
            .0
            .len(),
        2
    );
}
#[tokio::test]
async fn explicit_upcast_is_shared_by_recovery_and_projection_without_rewriting_history() {
    let f = Fixture::new().await;
    f.seed().await;
    let reopened = EventLog::connect(&f.url).await.unwrap();
    let before = serde_json::to_value(
        reopened
            .load_events(Balance::aggregate_type(), "one")
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    assert_eq!(before[0]["schema_version"], 1);
    assert_eq!(before[1]["schema_version"], 2);
    let loaded: LoadedAggregate<Balance> = SimpleAggregateManager::new(reopened.clone())
        .load(&Id("one".into()))
        .await
        .unwrap();
    assert_eq!(loaded.state().total, 10);
    let projection = Projector::new(f.reads.clone(), Totals::<0> { fail: false });
    assert_eq!(
        projection
            .catch_up(&reopened, &Id("one".into()))
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        projection
            .catch_up(&reopened, &Id("one".into()))
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        TotalRow::get_by_id(&mut f.reads.clone(), "0-one")
            .await
            .unwrap()
            .value,
        10
    );
    assert_eq!(
        before,
        serde_json::to_value(
            reopened
                .load_events(Balance::aggregate_type(), "one")
                .await
                .unwrap()
                .0
        )
        .unwrap()
    );
}
#[derive(Serialize, Deserialize)]
struct Plain;
impl EventPayload for Plain {
    fn event_type(&self) -> &'static str {
        "plain"
    }
}
#[test]
fn default_decoder_rejects_wrong_kind_and_revision_and_missing_revision_has_no_default() {
    assert!(matches!(
        Plain::decode("plain", 2, Value::Null),
        Err(CqrsError::EventFormat(_))
    ));
    assert!(matches!(
        Plain::decode("other", 1, Value::Null),
        Err(CqrsError::EventFormat(_))
    ));
    let new = NewEvent::from_payload(Plain, EventMetadata::default()).unwrap();
    let mut serialized = serde_json::to_value(new).unwrap();
    serialized.as_object_mut().unwrap().remove("schema_version");
    assert!(serde_json::from_value::<NewEvent>(serialized).is_err());
}
#[tokio::test]
async fn catch_up_failure_resumes_from_its_own_checkpoint_and_bad_formats_never_advance_it() {
    let f = Fixture::new().await;
    f.seed().await;
    let id = Id("one".into());
    let failed = Projector::new(f.reads.clone(), Totals::<0> { fail: true });
    assert!(failed.catch_up(&f.events, &id).await.is_err());
    assert_eq!(failed.last_applied(&id).await.unwrap(), 1);
    let independent = Projector::new(f.reads.clone(), Totals::<1> { fail: false });
    assert_eq!(independent.last_applied(&id).await.unwrap(), 0);
    independent.catch_up(&f.events, &id).await.unwrap();
    let retry = Projector::new(f.reads.clone(), Totals::<0> { fail: false });
    retry.catch_up(&f.events, &id).await.unwrap();
    assert_eq!(
        TotalRow::get_by_id(&mut f.reads.clone(), "0-one")
            .await
            .unwrap()
            .value,
        10
    );
    let mut next = NewEvent::from_payload(Credited { value: 2 }, EventMetadata::default()).unwrap();
    next.schema_version = 99;
    f.events
        .save_events(Balance::aggregate_type(), "one", &[next], 2)
        .await
        .unwrap();
    assert!(matches!(
        retry.catch_up(&f.events, &id).await,
        Err(CqrsError::EventFormat(_))
    ));
    assert_eq!(retry.last_applied(&id).await.unwrap(), 2);
}
#[tokio::test]
async fn offline_preflight_failure_has_no_deliveries_and_replay_error_retains_stream_and_progress()
{
    let f = Fixture::new().await;
    f.seed().await;
    let consumers =
        EventConsumers::new().with(Projector::new(f.reads.clone(), Totals::<0> { fail: true }));
    let plan = || {
        ReplayPlan::new()
            .with_aggregate::<Balance>()
            .with_page_size(NonZeroUsize::new(1).unwrap())
    };
    let prepared = plan().prepare(&f.events).await.unwrap();
    let error = prepared.run(&consumers).await.unwrap_err();
    assert_eq!(
        error.stream,
        Some((Balance::aggregate_type().into(), "one".into()))
    );
    assert_eq!(
        error.progress,
        ReplayProgress {
            streams: 0,
            events: 1
        }
    );
    assert!(matches!(error.source, CqrsError::Domain(_)));
    let retry =
        EventConsumers::new().with(Projector::new(f.reads.clone(), Totals::<0> { fail: false }));
    let done = plan()
        .prepare(&f.events)
        .await
        .unwrap()
        .run(&retry)
        .await
        .unwrap();
    assert_eq!(
        done,
        ReplayProgress {
            streams: 1,
            events: 2
        }
    );
    let extra = NewEvent::from_payload(Plain, EventMetadata::default()).unwrap();
    f.events
        .save_events("zz.unknown", "two", &[extra], 0)
        .await
        .unwrap();
    let result = plan().prepare(&f.events).await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("unknown stream accepted"),
    };
    assert_eq!(error.progress, ReplayProgress::default());
    assert_eq!(error.stream, Some(("zz.unknown".into(), "two".into())));
    assert!(matches!(error.source, CqrsError::EventFormat(_)));
}
#[tokio::test]
async fn driver_sources_survive_append_projection_and_old_schema_failures() {
    use std::error::Error;
    let f = Fixture::new().await;
    toasty::sql::query("DROP TABLE cqrs_events")
        .exec(&mut f.events.database())
        .await
        .unwrap();
    let engine = SimpleCqrs::new(f.events.clone());
    let error = engine
        .execute(&Id("one".into()), &Add(1), EventMetadata::default())
        .await
        .unwrap_err();
    let ExecutionError::NotCommitted { source } = error else {
        panic!("wrong boundary")
    };
    assert!(
        source
            .source()
            .unwrap()
            .downcast_ref::<toasty::Error>()
            .is_some()
    );
    let append = Fixture::new().await;
    let engine = SimpleCqrs::new(append.events.clone());
    engine
        .execute(&Id("one".into()), &Add(1), EventMetadata::default())
        .await
        .unwrap();
    toasty::sql::query("CREATE UNIQUE INDEX reject_append ON cqrs_events(event_type)")
        .exec(&mut append.events.database())
        .await
        .unwrap();
    let failed = engine
        .execute(&Id("one".into()), &Add(1), EventMetadata::default())
        .await
        .unwrap_err();
    let ExecutionError::CommitUnknown { source } = failed else {
        panic!("append outcome classification changed")
    };
    assert!(
        source
            .source()
            .unwrap()
            .downcast_ref::<toasty::Error>()
            .is_some()
    );
    assert_eq!(
        append
            .events
            .stream_version(Balance::aggregate_type(), "one")
            .await
            .unwrap(),
        1
    );
    let fresh = Fixture::new().await;
    fresh.seed().await;
    toasty::sql::query("DROP TABLE projection_checkpoints")
        .exec(&mut fresh.reads.clone())
        .await
        .unwrap();
    let projection = Projector::new(fresh.reads.clone(), Totals::<0> { fail: false });
    let error = projection
        .catch_up(&fresh.events, &Id("one".into()))
        .await
        .unwrap_err();
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<toasty::Error>()
            .is_some()
    );
    // Simulates an older database without a revision column. Loading fails; no implicit DDL/backfill.
    toasty::sql::query("ALTER TABLE cqrs_events DROP COLUMN schema_version")
        .exec(&mut fresh.events.database())
        .await
        .unwrap();
    assert!(matches!(
        fresh
            .events
            .load_events(Balance::aggregate_type(), "one")
            .await,
        Err(CqrsError::Storage { .. })
    ));
}

#[tokio::test]
async fn revision_storage_bounds_are_validated_without_inventing_defaults() {
    let f = Fixture::new().await;
    let mut invalid = NewEvent::from_payload(Plain, EventMetadata::default()).unwrap();
    invalid.schema_version = 0;
    assert!(matches!(
        f.events.save_events("test", "id", &[invalid], 0).await,
        Err(CqrsError::EventFormat(_))
    ));
    assert_eq!(f.events.stream_version("test", "id").await.unwrap(), 0);
    f.seed().await;
    for revision in [-1i64, 0, u32::MAX as i64 + 1] {
        toasty::sql::query(format!(
            "UPDATE cqrs_events SET schema_version = {revision}"
        ))
        .exec(&mut f.events.database())
        .await
        .unwrap();
        assert!(matches!(
            f.events.load_events(Balance::aggregate_type(), "one").await,
            Err(CqrsError::EventFormat(_))
        ));
    }
}
