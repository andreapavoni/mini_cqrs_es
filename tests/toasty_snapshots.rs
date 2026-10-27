#![cfg(any(feature = "turso", feature = "postgresql"))]

#[path = "support/database.rs"]
mod database;

use mini_cqrs_es::{
    Aggregate, AggregateManager, AggregateSnapshot, Command, Cqrs, CqrsError, EventConsumers,
    EventLog, EventMetadata, EventPayload, EventStore, LoadedAggregate, NewEvent, SimpleCqrs,
    SnapshotAggregateManager, SnapshotCache, SnapshotStore, toasty,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Counter<const N: u8> {
    value: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Credit(u64);

impl std::fmt::Display for Credit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("credited")
    }
}
impl EventPayload for Credit {
    fn event_type(&self) -> &'static str {
        "credited"
    }
}

impl<const N: u8> Aggregate for Counter<N> {
    type Id = String;
    type Event = Credit;

    fn aggregate_type() -> &'static str {
        match N {
            0 => "test.counter.alpha",
            _ => "test.counter.beta",
        }
    }

    async fn apply(&mut self, event: &Credit) {
        self.value += event.0;
    }
}

type Alpha = Counter<0>;

struct Database {
    events: EventLog,
    url: String,
    _directory: tempfile::TempDir,
}

impl Database {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let url = database::fresh_url(&directory.path().join("events.db"), false).await;
        let events = EventLog::connect(&url).await.unwrap();
        events.database().push_schema().await.unwrap();
        Self {
            events,
            url,
            _directory: directory,
        }
    }
}

async fn save<const N: u8>(store: &SnapshotCache, id: &str, version: u64, payload: Value) {
    store
        .save_snapshot(AggregateSnapshot::<Counter<N>>::from_parts(
            id.into(),
            version,
            payload,
        ))
        .await
        .unwrap();
}

async fn row<const N: u8>(store: &SnapshotCache, id: &str) -> AggregateSnapshot<Counter<N>> {
    store
        .load_snapshot::<Counter<N>>(&id.to_owned())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn snapshots_round_trip_after_reopen_and_isolate_types_and_ids() {
    let db = Database::new().await;
    let snapshots = db.events.snapshot_cache();
    assert!(
        snapshots
            .load_snapshot::<Alpha>(&"missing".into())
            .await
            .unwrap()
            .is_none()
    );
    let payload = json!({"value":0, "nested":[null, "caffè", {"flag":true}]});
    save::<0>(&snapshots, "same", 0, payload.clone()).await;
    save::<1>(&snapshots, "same", 7, json!({"value":70})).await;
    save::<0>(&snapshots, "other", i64::MAX as u64, json!({"value":90})).await;

    let reopened = SnapshotCache::connect(&db.url).await.unwrap();
    let zero = row::<0>(&reopened, "same").await;
    assert_eq!(zero.aggregate_id, "same");
    assert_eq!(zero.version, 0);
    assert_eq!(zero.payload(), &payload);
    assert_eq!(row::<1>(&reopened, "same").await.version, 7);
    assert_eq!(row::<0>(&reopened, "other").await.version, i64::MAX as u64);
    // Older accurate saves are allowed; both version and payload must be replaced.
    save::<0>(&snapshots, "other", 2, json!({"value":20})).await;
    let older = row::<0>(&reopened, "other").await;
    assert_eq!(older.version, 2);
    assert_eq!(older.payload(), &json!({"value":20}));
    assert_eq!(older.get_payload().unwrap().value, 20);
}

#[tokio::test]
async fn invalid_versions_and_json_are_errors_not_missing_snapshots() {
    let db = Database::new().await;
    let snapshots = db.events.snapshot_cache();
    save::<0>(&snapshots, "one", 1, json!({"value":1})).await;
    for version in [i64::MAX as u64 + 1, u64::MAX] {
        let result = snapshots
            .save_snapshot(AggregateSnapshot::<Alpha>::from_parts(
                "one".into(),
                version,
                json!({"value":99}),
            ))
            .await;
        assert!(matches!(result, Err(CqrsError::SnapshotStore(_))));
        assert_eq!(row::<0>(&snapshots, "one").await.version, 1);
        assert_eq!(
            row::<0>(&snapshots, "one").await.payload(),
            &json!({"value":1})
        );
    }
    let mut database = snapshots.database();
    toasty::sql::query("UPDATE cqrs_snapshots SET version = -1")
        .exec(&mut database)
        .await
        .unwrap();
    assert!(matches!(
        snapshots.load_snapshot::<Alpha>(&"one".into()).await,
        Err(CqrsError::SnapshotStore(_))
    ));
    toasty::sql::query("UPDATE cqrs_snapshots SET version = 1, payload = 'not json'")
        .exec(&mut database)
        .await
        .unwrap();
    assert!(matches!(
        snapshots.load_snapshot::<Alpha>(&"one".into()).await,
        Err(CqrsError::Serialization(_))
    ));
    // Valid JSON whose shape is incompatible with state must fail in the manager.
    save::<0>(&snapshots, "one", 1, json!({"value":"invalid"})).await;
    let manager = SnapshotAggregateManager::new(db.events.clone(), snapshots);
    let result: Result<LoadedAggregate<Alpha>, _> = manager.load(&"one".into()).await;
    assert!(matches!(result, Err(CqrsError::Serialization(_))));
}

#[tokio::test]
async fn failed_upsert_preserves_a_complete_previous_snapshot() {
    let db = Database::new().await;
    let snapshots = db.events.snapshot_cache();
    save::<0>(&snapshots, "one", 1, json!({"value":10})).await;
    save::<0>(&snapshots, "two", 2, json!({"value":20})).await;
    let mut database = snapshots.database();
    toasty::sql::query("CREATE UNIQUE INDEX injected_snapshot_failure ON cqrs_snapshots(version)")
        .exec(&mut database)
        .await
        .unwrap();
    let result = snapshots
        .save_snapshot(AggregateSnapshot::<Alpha>::from_parts(
            "one".into(),
            2,
            json!({"value":999}),
        ))
        .await;
    let error = result.unwrap_err();
    let CqrsError::Storage { source, .. } = error else {
        panic!("database source was lost")
    };
    assert!(source.downcast_ref::<toasty::Error>().is_some());
    let previous = row::<0>(&snapshots, "one").await;
    assert_eq!(previous.version, 1);
    assert_eq!(previous.payload(), &json!({"value":10}));
}

#[tokio::test]
async fn competing_saves_never_mix_state_and_version() {
    let db = Database::new().await;
    let a = db.events.snapshot_cache();
    let b = SnapshotCache::connect(&db.url).await.unwrap();
    let one = AggregateSnapshot::<Alpha>::from_parts("one".into(), 1, json!({"value":10}));
    let two = AggregateSnapshot::<Alpha>::from_parts("one".into(), 2, json!({"value":20}));
    let (first, second) = tokio::join!(a.save_snapshot(one), b.save_snapshot(two));
    assert!(first.is_ok() || second.is_ok()); // Contention need not be retried for a cache.
    let saved = row::<0>(&a, "one").await;
    assert!(matches!(
        (saved.version, saved.get_payload().unwrap().value),
        (1, 10) | (2, 20)
    ));
}

#[tokio::test]
async fn standalone_store_requires_explicit_schema_and_contains_no_event_models() {
    let directory = tempfile::tempdir().unwrap();
    let url = database::fresh_url(&directory.path().join("snapshots.db"), false).await;
    let snapshots = SnapshotCache::connect(&url).await.unwrap();
    let error = snapshots
        .load_snapshot::<Alpha>(&"one".into())
        .await
        .unwrap_err();
    assert!(matches!(error, CqrsError::Storage { .. })); // Missing table is not an absent row.
    snapshots.database().push_schema().await.unwrap();
    save::<0>(&snapshots, "one", 0, json!({"value":0})).await;
    let mut database = snapshots.database();
    assert!(
        toasty::sql::query("SELECT * FROM cqrs_events")
            .exec(&mut database)
            .await
            .is_err()
    );
    assert!(
        toasty::sql::query("SELECT * FROM projection_checkpoints")
            .exec(&mut database)
            .await
            .is_err()
    );
}

struct Add(u64);
impl Command for Add {
    type Aggregate = Alpha;
    async fn handle(&self, _: &Alpha) -> Result<Vec<Credit>, CqrsError> {
        Ok(vec![Credit(self.0)])
    }
}

#[tokio::test]
async fn command_snapshots_reopen_with_tail_recovery_and_can_be_discarded() {
    let db = Database::new().await;
    let snapshots = db.events.snapshot_cache();
    let id = "counter".to_owned();
    let cqrs = SimpleCqrs::from_parts(
        SnapshotAggregateManager::new(db.events.clone(), snapshots.clone()),
        db.events.clone(),
        EventConsumers::new(),
    );
    cqrs.execute(&id, &Add(10), EventMetadata::default())
        .await
        .unwrap();
    let saved = row::<0>(&snapshots, &id).await;
    assert_eq!(saved.version, 1);
    assert_eq!(saved.get_payload().unwrap().value, 10);
    // Emulate a later append whose projection/consumer failed before snapshot save.
    db.events
        .save_events(
            Alpha::aggregate_type(),
            &id,
            &[NewEvent::from_payload(Credit(20), EventMetadata::default()).unwrap()],
            1,
        )
        .await
        .unwrap();
    let reopened_events = EventLog::connect(&db.url).await.unwrap();
    let manager =
        SnapshotAggregateManager::new(reopened_events.clone(), reopened_events.snapshot_cache());
    let recovered: LoadedAggregate<Alpha> = manager.load(&id).await.unwrap();
    assert_eq!(recovered.version(), 2);
    assert_eq!(recovered.state().value, 30);
    let before = serde_json::to_value(
        reopened_events
            .load_events(Alpha::aggregate_type(), &id)
            .await
            .unwrap()
            .0,
    )
    .unwrap();
    // Snapshots are disposable; their deletion cannot delete the event log.
    toasty::sql::query("DELETE FROM cqrs_snapshots")
        .exec(&mut snapshots.database())
        .await
        .unwrap();
    let recovered: LoadedAggregate<Alpha> = manager.load(&id).await.unwrap();
    assert_eq!(recovered.version(), 2);
    assert_eq!(recovered.state().value, 30);
    assert_eq!(
        before,
        serde_json::to_value(
            reopened_events
                .load_events(Alpha::aggregate_type(), &id)
                .await
                .unwrap()
                .0
        )
        .unwrap()
    );
    let receipt = cqrs
        .execute(&id, &Add(5), EventMetadata::default())
        .await
        .unwrap();
    assert_eq!(receipt.version, 3);
    assert_eq!(
        row::<0>(&snapshots, &id).await.get_payload().unwrap().value,
        35
    );
}
