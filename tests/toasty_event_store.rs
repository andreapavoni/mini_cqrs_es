#![cfg(any(feature = "turso", feature = "postgresql"))]

#[path = "support/database.rs"]
mod database;

use mini_cqrs_es::{CqrsError, EventLog, EventMetadata, EventStore, NewEvent, StoredEvent, toasty};
use serde_json::{Value, json};

struct Database {
    store: EventLog,
    url: String,
    _directory: tempfile::TempDir,
}

impl Database {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let url = database::fresh_url(&directory.path().join("events.db"), false).await;
        let store = EventLog::connect(&url).await.unwrap();
        store.database().push_schema().await.unwrap();
        Self {
            store,
            url,
            _directory: directory,
        }
    }
}

fn event(value: Value) -> NewEvent {
    NewEvent {
        event_type: "changed".into(),
        schema_version: 1,
        payload: value,
        metadata: EventMetadata {
            command_id: Some("command".into()),
            correlation_id: Some("correlation".into()),
            causation_id: Some("cause".into()),
            actor: Some("actor".into()),
            tenant_id: Some("tenant".into()),
            extra: [("nested".into(), json!({"unicode": "caffè", "null": null}))].into(),
        },
        timestamp: "2026-10-19T10:11:12.123456789Z".parse().unwrap(),
    }
}

fn envelopes(events: &[StoredEvent]) -> Value {
    serde_json::to_value(events).unwrap()
}

fn conflict(error: CqrsError, expected: u64, actual: u64) {
    assert!(
        matches!(error, CqrsError::Conflict { expected_version, actual_version }
        if expected_version == expected && actual_version == actual)
    );
}

#[path = "support/common_game.rs"]
mod common_game;
#[path = "support/common_hotel.rs"]
mod common_hotel;
#[path = "support/read_store.rs"]
mod read_store;

#[tokio::test]
async fn execution_metadata_and_receipt_match_reopened_history() {
    use common_hotel::{CmdInitializeHotel, HotelAggregate, HotelId};
    use mini_cqrs_es::{Aggregate, Cqrs, EventConsumers, SimpleAggregateManager, SimpleCqrs};

    let db = Database::new().await;
    let id = HotelId::new(42);
    let metadata = event(json!(null)).metadata;
    let receipt = {
        let cqrs = SimpleCqrs::from_parts(
            SimpleAggregateManager::new(db.store.clone()),
            db.store.clone(),
            EventConsumers::new(),
        );
        cqrs.execute(&id, &CmdInitializeHotel { room_count: 3 }, metadata.clone())
            .await
            .unwrap()
    };
    let reopened = EventLog::connect(&db.url).await.unwrap();
    assert_eq!(HotelAggregate::aggregate_type(), "example.hotel");
    let (history, tip) = reopened
        .load_events(HotelAggregate::aggregate_type(), &id.to_string())
        .await
        .unwrap();
    assert_eq!(receipt.aggregate_id, id);
    assert_eq!(receipt.version, tip);
    assert_eq!(receipt.version, 1);
    assert_eq!(history.len(), 1);
    assert_eq!(receipt.event_ids, vec![history[0].id.clone()]);
    assert_eq!(
        serde_json::to_value(&history[0].metadata).unwrap(),
        serde_json::to_value(metadata).unwrap()
    );
}

#[tokio::test]
async fn replay_checks_state_and_version_even_after_a_consumer_error() {
    use common_hotel::{CmdCheckIn, CmdInitializeHotel, HotelAggregate, HotelId, RoomState};
    use mini_cqrs_es::{
        AggregateManager, Cqrs, EventConsumer, EventConsumers, ExecutionError, LoadedAggregate,
        PostCommitStage, SimpleAggregateManager, SimpleCqrs,
    };

    struct FailingConsumer;
    impl EventConsumer for FailingConsumer {
        async fn process(&self, _: &StoredEvent) -> Result<(), CqrsError> {
            Err(CqrsError::domain("projection failed"))
        }
    }

    let db = Database::new().await;
    let id = HotelId::new(42);
    let cqrs = SimpleCqrs::from_parts(
        SimpleAggregateManager::new(db.store.clone()),
        db.store.clone(),
        EventConsumers::new(),
    );
    cqrs.execute(
        &id,
        &CmdInitializeHotel { room_count: 3 },
        EventMetadata::default(),
    )
    .await
    .unwrap();
    let failing = SimpleCqrs::from_parts(
        SimpleAggregateManager::new(db.store.clone()),
        db.store.clone(),
        EventConsumers::new().with(FailingConsumer),
    );
    assert!(matches!(
        failing
            .execute(
                &id,
                &CmdCheckIn {
                    room_number: 1,
                    guest_name: "Alice".into()
                },
                EventMetadata::default()
            )
            .await,
        Err(ExecutionError::Committed {
            stage: PostCommitStage::Consumers,
            source: CqrsError::Domain(_),
            ..
        })
    ));
    let reopened = EventLog::connect(&db.url).await.unwrap();
    let manager = SimpleAggregateManager::new(reopened);
    let hotel: LoadedAggregate<HotelAggregate> = manager.load(&id).await.unwrap();
    assert_eq!(hotel.id(), &id);
    assert_eq!(hotel.version(), 2);
    assert_eq!(hotel.state().rooms.len(), 3);
    assert_eq!(
        hotel.state().rooms.get(&1),
        Some(&RoomState::Occupied {
            guest_name: "Alice".into()
        })
    );
}

#[cfg(feature = "turso")]
#[tokio::test]
async fn a_locked_writer_forces_retry_and_exhaustion_is_a_storage_error() {
    if database::postgres() {
        return;
    }
    let db = Database::new().await;
    let writer = EventLog::connect(&db.url).await.unwrap();
    let mut database = db.store.database();
    let mut lock = database.transaction().await.unwrap();
    toasty::sql::statement("INSERT INTO cqrs_events (id, aggregate_type, aggregate_id, version, event_type, schema_version, payload, metadata, timestamp) VALUES ('lock', 'A', '1', 1, 'changed', 1, 'null', '{}', '2026-10-19T00:00:00Z')")
        .exec(&mut lock).await.unwrap();
    let batch = [event(json!("contender"))];
    let append = writer.save_events("A", "1", &batch, 0);
    tokio::pin!(append);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut append)
            .await
            .is_err()
    );
    lock.commit().await.unwrap();
    conflict(append.await.unwrap_err(), 0, 1);

    let mut lock = database.transaction().await.unwrap();
    toasty::sql::statement("UPDATE cqrs_events SET event_type = 'locked' WHERE id = 'lock'")
        .exec(&mut lock)
        .await
        .unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        writer.save_events("A", "1", &batch, 1),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(CqrsError::Storage { .. })));
    lock.rollback().await.unwrap();
    assert_eq!(db.store.load_events("A", "1").await.unwrap().1, 1);
}

#[tokio::test]
async fn empty_streams_and_empty_appends_check_expected_version() {
    let db = Database::new().await;
    let (events, version) = db.store.load_events("A", "1").await.unwrap();
    assert!(events.is_empty());
    assert_eq!(version, 0);
    assert!(
        db.store
            .save_events("A", "1", &[], 0)
            .await
            .unwrap()
            .is_empty()
    );
    conflict(
        db.store.save_events("A", "1", &[], 1).await.unwrap_err(),
        1,
        0,
    );
    db.store
        .save_events("A", "1", &[event(json!(1))], 0)
        .await
        .unwrap();
    assert!(
        db.store
            .save_events("A", "1", &[], 1)
            .await
            .unwrap()
            .is_empty()
    );
    conflict(
        db.store.save_events("A", "1", &[], 0).await.unwrap_err(),
        0,
        1,
    );
}

#[tokio::test]
async fn ordered_envelopes_survive_reopen_and_keep_history() {
    let db = Database::new().await;
    let original = vec![
        event(json!({"array": [null, true, "✓"], "large": u64::MAX})),
        event(Value::Null),
    ];
    let first = db
        .store
        .save_events("Hotel", "héllo/1", &original, 0)
        .await
        .unwrap();
    assert_eq!(
        first.iter().map(|event| event.version).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_ne!(first[0].id, first[1].id);
    for (stored, new) in first.iter().zip(&original) {
        assert_eq!(stored.payload, new.payload);
        assert_eq!(
            serde_json::to_value(&stored.metadata).unwrap(),
            serde_json::to_value(&new.metadata).unwrap()
        );
        assert_eq!(stored.timestamp, new.timestamp);
        assert_eq!(stored.event_type, new.event_type);
        assert_eq!(stored.aggregate_type, "Hotel");
        assert_eq!(stored.aggregate_id, "héllo/1");
        assert_eq!(stored.global_sequence, None);
    }
    db.store
        .save_events("Hotel", "héllo/1", &[event(json!(3))], 2)
        .await
        .unwrap();
    let reopened = EventLog::connect(&db.url).await.unwrap();
    let (loaded, version) = reopened.load_events("Hotel", "héllo/1").await.unwrap();
    assert_eq!(version, 3);
    assert_eq!(envelopes(&loaded[..2]), envelopes(&first));
    assert_eq!(loaded[2].version, 3);
}

#[tokio::test]
async fn streams_are_isolated_and_stale_writes_leave_no_rows() {
    let db = Database::new().await;
    for (kind, id) in [("A", "1"), ("A", "2"), ("B", "1")] {
        db.store
            .save_events(kind, id, &[event(json!([kind, id]))], 0)
            .await
            .unwrap();
    }
    conflict(
        db.store
            .save_events("A", "1", &[event(json!("stale")), event(json!("stale"))], 0)
            .await
            .unwrap_err(),
        0,
        1,
    );
    for (kind, id) in [("A", "1"), ("A", "2"), ("B", "1")] {
        let (events, version) = db.store.load_events(kind, id).await.unwrap();
        assert_eq!(version, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, json!([kind, id]));
    }
}

#[tokio::test]
async fn independent_writers_commit_only_one_complete_batch() {
    let db = Database::new().await;
    let second = EventLog::connect(&db.url).await.unwrap();
    for expected in [0, 2] {
        let left = vec![event(json!("left-1")), event(json!("left-2"))];
        let right = vec![event(json!("right-1")), event(json!("right-2"))];
        let (a, b) = tokio::join!(
            db.store.save_events("A", "1", &left, expected),
            second.save_events("A", "1", &right, expected),
        );
        let (winner, loser) = match (a, b) {
            (Ok(winner), Err(loser)) | (Err(loser), Ok(winner)) => (winner, loser),
            result => panic!("expected one committed batch and one rejected writer: {result:?}"),
        };
        if database::postgres() {
            assert!(matches!(
                loser,
                CqrsError::Conflict { .. } | CqrsError::Storage { .. }
            ));
        } else {
            conflict(loser, expected, expected + 2);
        }
        let (loaded, version) = db.store.load_events("A", "1").await.unwrap();
        assert_eq!(version, expected + 2);
        assert_eq!(loaded.len(), version as usize);
        assert_eq!(envelopes(&loaded[expected as usize..]), envelopes(&winner));
        assert_eq!(
            loaded.iter().map(|event| event.version).collect::<Vec<_>>(),
            (1..=version).collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn later_insert_failure_rolls_back_the_inserted_prefix() {
    let db = Database::new().await;
    let mut database = db.store.database();
    // A database-side failure on the second record proves transaction atomicity.
    toasty::sql::statement("CREATE UNIQUE INDEX reject_second_event ON cqrs_events(event_type)")
        .exec(&mut database)
        .await
        .unwrap();
    let good = event(json!(1));
    let bad = event(json!(2));
    assert!(matches!(
        db.store.save_events("A", "1", &[good, bad], 0).await,
        Err(CqrsError::Storage { .. })
    ));
    let (events, version) = db.store.load_events("A", "1").await.unwrap();
    assert!(events.is_empty());
    assert_eq!(version, 0);
    db.store
        .save_events("A", "1", &[event(json!("recovered"))], 0)
        .await
        .unwrap();
}

#[tokio::test]
async fn malformed_records_and_version_overflow_are_errors() {
    let db = Database::new().await;
    for expected in [i64::MAX as u64, u64::MAX] {
        assert!(matches!(
            db.store
                .save_events("A", "1", &[event(json!(1))], expected)
                .await,
            Err(CqrsError::EventStore(_))
        ));
    }
    assert!(db.store.load_events("A", "1").await.unwrap().0.is_empty());
    db.store
        .save_events("A", "1", &[event(json!(1))], 0)
        .await
        .unwrap();
    let mut database = db.store.database();
    toasty::sql::statement("UPDATE cqrs_events SET timestamp = 'bad'")
        .exec(&mut database)
        .await
        .unwrap();
    assert!(matches!(
        db.store.load_events("A", "1").await,
        Err(CqrsError::EventStore(_))
    ));
    toasty::sql::statement(
        "UPDATE cqrs_events SET timestamp = '2026-10-19T00:00:00Z', payload = '{'",
    )
    .exec(&mut database)
    .await
    .unwrap();
    assert!(matches!(
        db.store.load_events("A", "1").await,
        Err(CqrsError::Serialization(_))
    ));
}

#[tokio::test]
async fn bounded_event_pages_preserve_envelopes_and_exclude_appends_after_opening() {
    use mini_cqrs_es::EventStreamReader;
    use std::num::NonZeroUsize;
    let size = NonZeroUsize::new(2).unwrap();
    let db = Database::new().await;
    let batch: Vec<_> = (1..=5).map(|value| event(json!(value))).collect();
    let saved = db.store.save_events("A", "1", &batch, 0).await.unwrap();
    db.store
        .save_events("B", "1", &[event(json!(99))], 0)
        .await
        .unwrap();
    db.store
        .save_events("A", "2", &[event(json!(88))], 0)
        .await
        .unwrap();
    assert_eq!(db.store.stream_version("missing", "1").await.unwrap(), 0);
    let reopened = EventLog::connect(&db.url).await.unwrap();
    let mut reader = EventStreamReader::open(&reopened, "A", "1", 1, size)
        .await
        .unwrap();
    assert_eq!(reader.through_version(), 5);
    assert_eq!(
        envelopes(&reader.next_page().await.unwrap().unwrap()),
        envelopes(&saved[1..3])
    );
    let late = db
        .store
        .save_events("A", "1", &[event(json!(6))], 5)
        .await
        .unwrap();
    assert_eq!(
        envelopes(&reader.next_page().await.unwrap().unwrap()),
        envelopes(&saved[3..5])
    );
    assert!(reader.next_page().await.unwrap().is_none());
    assert_eq!(reader.position(), 5);
    let mut tail = EventStreamReader::open(&reopened, "A", "1", 5, size)
        .await
        .unwrap();
    assert_eq!(tail.through_version(), 6);
    assert_eq!(
        envelopes(&tail.next_page().await.unwrap().unwrap()),
        envelopes(&late)
    );
    assert!(tail.next_page().await.unwrap().is_none());
    assert_eq!(
        envelopes(
            &db.store
                .load_events_after("A", "1", 0, 5, size)
                .await
                .unwrap()
        ),
        envelopes(&saved[..2])
    );
    assert!(
        db.store
            .load_events_after("A", "1", 5, 5, size)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.store
            .load_events_after("A", "1", 5, 4, size)
            .await
            .is_err()
    );
    assert!(
        db.store
            .load_events_after("A", "1", 0, i64::MAX as u64 + 1, size)
            .await
            .is_err()
    );
    assert!(
        db.store
            .load_events_after("A", "1", u64::MAX, u64::MAX, size)
            .await
            .is_err()
    );
    if usize::BITS == 64 {
        assert!(
            db.store
                .load_events_after("A", "1", 0, 1, NonZeroUsize::new(usize::MAX).unwrap())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn stream_key_pages_are_unique_and_do_not_depend_on_valid_first_events_or_payloads() {
    use std::num::NonZeroUsize;
    let db = Database::new().await;
    let expected: Vec<(String, String)> = [
        ("", ""),
        ("A", "A"),
        ("A", "a"),
        ("A", "é"),
        ("B", "a"),
        ("a", "A"),
        ("é", "🦀"),
    ]
    .into_iter()
    .map(|(kind, id)| (kind.into(), id.into()))
    .collect();
    for (kind, id) in &expected {
        db.store
            .save_events(kind, id, &[event(json!(1)), event(json!(2))], 0)
            .await
            .unwrap();
    }
    let mut database = db.store.database();
    toasty::sql::statement(
        "DELETE FROM cqrs_events WHERE aggregate_type = 'A' AND aggregate_id = 'a' AND version = 1",
    )
    .exec(&mut database)
    .await
    .unwrap();
    toasty::sql::statement("UPDATE cqrs_events SET payload = 'invalid json' WHERE aggregate_type = 'A' AND aggregate_id = 'a'")
        .exec(&mut database).await.unwrap();
    let size = NonZeroUsize::new(2).unwrap();
    let mut keys = Vec::new();
    let mut cursor = None;
    loop {
        let page = db
            .store
            .list_streams_after(cursor.as_ref(), size)
            .await
            .unwrap();
        assert!(page.len() <= size.get());
        if page.is_empty() {
            break;
        }
        cursor = page.last().cloned();
        keys.extend(page);
    }
    // Compare keyset traversal with this backend's own full ordering, not Rust's
    // UTF-8 ordering: PostgreSQL collation is database configuration.
    let fields = mini_cqrs_es::persistence::EventRecord::fields();
    let mut ordered = mini_cqrs_es::persistence::EventRecord::all()
        .order_by((fields.aggregate_type().asc(), fields.aggregate_id().asc()))
        .select((fields.aggregate_type(), fields.aggregate_id()))
        .exec(&mut database)
        .await
        .unwrap();
    ordered.dedup();
    assert_eq!(keys, ordered);
    let discovered: std::collections::BTreeSet<_> = keys.into_iter().collect();
    assert_eq!(discovered, expected.into_iter().collect());
    assert!(matches!(
        db.store.load_events_after("A", "a", 1, 2, size).await,
        Err(CqrsError::Serialization(_))
    ));
}
