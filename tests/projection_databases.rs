#![cfg(any(feature = "turso", feature = "postgresql"))]
#[path = "support/database.rs"]
mod database;

#[path = "support/common_game.rs"]
mod common_game;
#[path = "support/common_hotel.rs"]
mod common_hotel;
#[path = "support/read_store.rs"]
mod read_store;

use common_game::*;
use common_hotel::*;
use mini_cqrs_es::ProjectionCheckpoint;
use mini_cqrs_es::{
    Aggregate, Cqrs, EventConsumers, EventLog, EventMetadata, EventStore, NewEvent, Query,
    SimpleAggregateManager, SimpleCqrs, replay_stream, toasty,
};
use read_store::Databases;

type Engine = SimpleCqrs<EventLog, SimpleAggregateManager<EventLog>>;

fn engine(fixture: &Databases) -> Engine {
    SimpleCqrs::from_parts(
        SimpleAggregateManager::new(fixture.events.clone()),
        fixture.events.clone(),
        read_store::projections(&fixture.reads),
    )
}

fn start_game() -> CmdStartGame {
    CmdStartGame {
        player_1: Player {
            id: "alice".into(),
            points: 0,
        },
        player_2: Player {
            id: "bob".into(),
            points: 0,
        },
        goal: 3,
    }
}

async fn history(events: &EventLog) -> serde_json::Value {
    let mut result = Vec::new();
    for (kind, id) in events.list_streams().await.unwrap() {
        result.push(events.load_events(&kind, &id).await.unwrap().0);
    }
    serde_json::to_value(result).unwrap()
}

#[tokio::test]
async fn wiped_read_database_is_rebuilt_for_every_hotel_and_game() {
    let fixture = Databases::temporary().await.unwrap();
    let cqrs = engine(&fixture);
    let hotels = [HotelId::new(1), HotelId::new(2)];
    let games = [GameId::new("game-1"), GameId::new("game-2")];
    for id in &hotels {
        cqrs.execute(
            id,
            &CmdInitializeHotel { room_count: 3 },
            EventMetadata::default(),
        )
        .await
        .unwrap();
    }
    cqrs.execute(
        &hotels[0],
        &CmdCheckIn {
            room_number: 1,
            guest_name: "Alice".into(),
        },
        EventMetadata::default(),
    )
    .await
    .unwrap();
    cqrs.execute(
        &hotels[1],
        &CmdCheckIn {
            room_number: 1,
            guest_name: "Bob".into(),
        },
        EventMetadata::default(),
    )
    .await
    .unwrap();
    cqrs.execute(
        &hotels[1],
        &CmdCheckOut { room_number: 1 },
        EventMetadata::default(),
    )
    .await
    .unwrap();
    for id in &games {
        cqrs.execute(id, &start_game(), EventMetadata::default())
            .await
            .unwrap();
    }
    let attack = CmdAttackPlayer {
        attacker: start_game().player_1,
    };
    for _ in 0..3 {
        cqrs.execute(&games[0], &attack, EventMetadata::default())
            .await
            .unwrap();
    }
    cqrs.execute(&games[1], &attack, EventMetadata::default())
        .await
        .unwrap();
    let mut expected_hotels = Vec::new();
    for id in &hotels {
        expected_hotels.push(
            GetHotelStateQuery::new(id.clone(), fixture.reads.clone())
                .apply()
                .await
                .unwrap(),
        );
    }
    assert_eq!(
        expected_hotels[0].rooms[&1],
        RoomState::Occupied {
            guest_name: "Alice".into()
        }
    );
    assert_eq!(expected_hotels[1].rooms[&1], RoomState::Free);
    let mut expected_games = Vec::new();
    for id in &games {
        expected_games.push(
            GetGameQuery::new(id.clone(), fixture.reads.clone())
                .apply()
                .await
                .unwrap()
                .unwrap(),
        );
    }
    verify_game_result(
        &expected_games[0],
        3,
        0,
        3,
        GameStatus::Winner(Player {
            id: "alice".into(),
            points: 3,
        }),
    );
    verify_game_result(&expected_games[1], 1, 0, 3, GameStatus::Playing);
    assert_eq!(fixture.events.list_streams().await.unwrap().len(), 4);
    let before = history(&fixture.events).await;

    // A durable supporting table belongs only to the source database.
    let mut event_db = fixture.events.database();
    toasty::sql::statement("CREATE TABLE durable_marker (value TEXT NOT NULL)")
        .exec(&mut event_db)
        .await
        .unwrap();
    toasty::sql::statement("INSERT INTO durable_marker VALUES ('retain me')")
        .exec(&mut event_db)
        .await
        .unwrap();
    let marker = format!(
        "{:?}",
        toasty::sql::query("SELECT * FROM durable_marker")
            .exec(&mut event_db)
            .await
            .unwrap()
    );
    assert!(
        toasty::sql::query("SELECT * FROM projection_checkpoints")
            .exec(&mut event_db)
            .await
            .is_err()
    );
    let mut read_db = fixture.reads.clone();
    assert!(
        toasty::sql::query("SELECT * FROM cqrs_snapshots")
            .exec(&mut read_db)
            .await
            .is_err()
    );
    assert!(
        toasty::sql::query("SELECT * FROM cqrs_events")
            .exec(&mut read_db)
            .await
            .is_err()
    );
    assert!(
        toasty::sql::query("SELECT * FROM durable_marker")
            .exec(&mut read_db)
            .await
            .is_err()
    );
    drop(read_db);
    drop(cqrs); // Quiesce command delivery and release all active read handles.
    let Databases {
        directory,
        events,
        reads,
    } = fixture;
    drop(reads);
    std::fs::remove_file(directory.path().join("read_models.db")).unwrap();
    let rebuilt_path = directory.path().join("rebuilt.db");
    let rebuilt = read_store::rebuild(&events, &rebuilt_path).await.unwrap();
    // Reopen the completed replacement before exposing it to queries.
    drop(rebuilt);
    let rebuilt = read_store::open(&rebuilt_path).await.unwrap();
    for (id, expected) in hotels.iter().zip(expected_hotels) {
        assert_eq!(
            GetHotelStateQuery::new(id.clone(), rebuilt.clone())
                .apply()
                .await
                .unwrap(),
            expected
        );
    }
    for (id, expected) in games.iter().zip(expected_games) {
        assert_eq!(
            GetGameQuery::new(id.clone(), rebuilt.clone())
                .apply()
                .await
                .unwrap()
                .unwrap(),
            expected
        );
    }
    let checkpoints = ProjectionCheckpoint::all()
        .exec(&mut rebuilt.clone())
        .await
        .unwrap();
    assert_eq!(checkpoints.len(), 4);
    for checkpoint in checkpoints {
        let (_, version) = events
            .load_events(&checkpoint.aggregate_type, &checkpoint.aggregate_id)
            .await
            .unwrap();
        assert_eq!(checkpoint.last_version as u64, version);
    }
    assert_eq!(history(&events).await, before);
    assert_eq!(
        format!(
            "{:?}",
            toasty::sql::query("SELECT * FROM durable_marker")
                .exec(&mut event_db)
                .await
                .unwrap()
        ),
        marker
    );
}

#[tokio::test]
async fn duplicate_delivery_is_ignored_and_gaps_do_not_advance_state() {
    let fixture = Databases::temporary().await.unwrap();
    // Persist without live projections, then exercise ordered recovery explicitly.
    let cqrs = SimpleCqrs::from_parts(
        SimpleAggregateManager::new(fixture.events.clone()),
        fixture.events.clone(),
        EventConsumers::new(),
    );
    let id = GameId::new("game");
    cqrs.execute(&id, &start_game(), EventMetadata::default())
        .await
        .unwrap();
    for _ in 0..2 {
        cqrs.execute(
            &id,
            &CmdAttackPlayer {
                attacker: start_game().player_1,
            },
            EventMetadata::default(),
        )
        .await
        .unwrap();
    }
    let kind = GameAggregate::aggregate_type();
    let (events, _) = fixture
        .events
        .load_events(kind, &id.to_string())
        .await
        .unwrap();
    let consumers = read_store::projections(&fixture.reads);
    consumers.process(&events[0]).await.unwrap();
    assert!(consumers.process(&events[2]).await.is_err());
    let query = GetGameQuery::new(id.clone(), fixture.reads.clone());
    assert_eq!(query.apply().await.unwrap().unwrap().player_1.points, 0);
    consumers.process(&events[1]).await.unwrap();
    consumers.process(&events[1]).await.unwrap();
    assert_eq!(query.apply().await.unwrap().unwrap().player_1.points, 1);
    replay_stream(&fixture.events, kind, &id.to_string(), &consumers)
        .await
        .unwrap();
    replay_stream(&fixture.events, kind, &id.to_string(), &consumers)
        .await
        .unwrap();
    assert_eq!(query.apply().await.unwrap().unwrap().player_1.points, 2);
    let rows = ProjectionCheckpoint::all()
        .exec(&mut fixture.reads.clone())
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].last_version, 3);
}

#[tokio::test]
async fn failed_projection_rolls_back_rows_and_checkpoint_after_durable_append() {
    let fixture = Databases::temporary().await.unwrap();
    let mut reads = fixture.reads.clone();
    // The first room succeeds and the second fails, after hotel row mutations.
    toasty::sql::statement("CREATE UNIQUE INDEX injected_failure ON room_read_models(hotel_id)")
        .exec(&mut reads)
        .await
        .unwrap();
    let cqrs = engine(&fixture);
    let id = HotelId::new(1);
    let error = cqrs
        .execute(
            &id,
            &CmdInitializeHotel { room_count: 3 },
            EventMetadata::default(),
        )
        .await
        .unwrap_err();
    let mini_cqrs_es::ExecutionError::Committed {
        source: mini_cqrs_es::CqrsError::Storage { source, .. },
        ..
    } = error
    else {
        panic!("post-commit driver source was lost")
    };
    assert!(source.downcast_ref::<toasty::Error>().is_some());
    assert!(
        HotelReadModel::all()
            .exec(&mut reads)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        RoomReadModel::all()
            .exec(&mut reads)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        ProjectionCheckpoint::all()
            .exec(&mut reads)
            .await
            .unwrap()
            .is_empty()
    );
    let kind = HotelAggregate::aggregate_type();
    let before = fixture
        .events
        .load_events(kind, &id.to_string())
        .await
        .unwrap();
    assert_eq!(before.1, 1);
    toasty::sql::statement("DROP INDEX injected_failure")
        .exec(&mut reads)
        .await
        .unwrap();
    replay_stream(
        &fixture.events,
        kind,
        &id.to_string(),
        &read_store::projections(&reads),
    )
    .await
    .unwrap();
    assert_eq!(
        GetHotelStateQuery::new(id.clone(), reads.clone())
            .apply()
            .await
            .unwrap()
            .rooms
            .len(),
        3
    );
    assert_eq!(
        ProjectionCheckpoint::all().exec(&mut reads).await.unwrap()[0].last_version,
        1
    );
    let after = fixture
        .events
        .load_events(kind, &id.to_string())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(before.0).unwrap(),
        serde_json::to_value(after.0).unwrap()
    );
}

#[tokio::test]
async fn unsuccessful_rebuild_never_replaces_or_resets_existing_files() {
    let fixture = Databases::temporary().await.unwrap();
    let cqrs = engine(&fixture);
    let id = HotelId::new(1);
    cqrs.execute(
        &id,
        &CmdInitializeHotel { room_count: 3 },
        EventMetadata::default(),
    )
    .await
    .unwrap();
    let query = GetHotelStateQuery::new(id.clone(), fixture.reads.clone());
    let before = query.apply().await.unwrap();
    for name in ["events.db", "read_models.db"] {
        assert!(
            read_store::rebuild(&fixture.events, &fixture.directory.path().join(name))
                .await
                .is_err()
        );
    }
    // Valid JSON with an invalid domain payload causes replay failure in a new target.
    fixture
        .events
        .save_events(
            HotelAggregate::aggregate_type(),
            &id.to_string(),
            &[NewEvent {
                event_type: "invalid".into(),
                schema_version: 1,
                payload: serde_json::json!({"unknown": true}),
                metadata: EventMetadata::default(),
                timestamp: chrono::Utc::now(),
            }],
            1,
        )
        .await
        .unwrap();
    assert!(
        read_store::rebuild(&fixture.events, &fixture.directory.path().join("failed.db"))
            .await
            .is_err()
    );
    assert_eq!(query.apply().await.unwrap(), before);
    assert_eq!(
        fixture
            .events
            .load_events(HotelAggregate::aggregate_type(), &id.to_string())
            .await
            .unwrap()
            .1,
        2
    );
}

#[tokio::test]
async fn discovery_skips_histories_and_rejects_unknown_types_before_target_creation() {
    let fixture = Databases::temporary().await.unwrap();
    assert!(fixture.events.list_streams().await.unwrap().is_empty());
    let event = NewEvent {
        event_type: "sample".into(),
        schema_version: 1,
        payload: serde_json::Value::Null,
        metadata: EventMetadata::default(),
        timestamp: chrono::Utc::now(),
    };
    fixture
        .events
        .save_events("A", "same", &vec![event.clone(); 260], 0)
        .await
        .unwrap();
    fixture
        .events
        .save_events("A", "second", std::slice::from_ref(&event), 0)
        .await
        .unwrap();
    fixture
        .events
        .save_events("B", "same", &[event], 0)
        .await
        .unwrap();
    assert_eq!(
        fixture.events.list_streams().await.unwrap(),
        [
            ("A".into(), "same".into()),
            ("A".into(), "second".into()),
            ("B".into(), "same".into())
        ]
    );
    assert!(
        read_store::rebuild(
            &fixture.events,
            &fixture.directory.path().join("unknown.db")
        )
        .await
        .is_err()
    );
    assert!(!fixture.directory.path().join("unknown.db").exists());
}

struct PageOnlyStore {
    store: EventLog,
    catalog_reads: std::sync::atomic::AtomicUsize,
}

impl EventStore for PageOnlyStore {
    async fn save_events(
        &self,
        _: &str,
        _: &str,
        _: &[NewEvent],
        _: u64,
    ) -> Result<Vec<mini_cqrs_es::StoredEvent>, mini_cqrs_es::CqrsError> {
        panic!("rebuilding must not append events")
    }
    async fn stream_version(&self, kind: &str, id: &str) -> Result<u64, mini_cqrs_es::CqrsError> {
        self.store.stream_version(kind, id).await
    }
    async fn load_events_after(
        &self,
        kind: &str,
        id: &str,
        after: u64,
        through: u64,
        limit: std::num::NonZeroUsize,
    ) -> Result<Vec<mini_cqrs_es::StoredEvent>, mini_cqrs_es::CqrsError> {
        self.store
            .load_events_after(kind, id, after, through, limit)
            .await
    }
    async fn list_streams_after(
        &self,
        after: Option<&mini_cqrs_es::StreamKey>,
        _: std::num::NonZeroUsize,
    ) -> Result<Vec<mini_cqrs_es::StreamKey>, mini_cqrs_es::CqrsError> {
        self.catalog_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // A short nonempty catalog page is valid; consumers must continue seeking.
        self.store
            .list_streams_after(after, std::num::NonZeroUsize::new(1).unwrap())
            .await
    }
    async fn load_events(
        &self,
        _: &str,
        _: &str,
    ) -> Result<(Vec<mini_cqrs_es::StoredEvent>, u64), mini_cqrs_es::CqrsError> {
        panic!("rebuild must not collect a whole stream")
    }
    async fn list_streams(&self) -> Result<Vec<mini_cqrs_es::StreamKey>, mini_cqrs_es::CqrsError> {
        panic!("rebuild must not collect the entire catalog")
    }
}

#[tokio::test]
async fn rebuild_uses_only_bounded_catalog_and_event_reads_across_short_pages() {
    let fixture = Databases::temporary().await.unwrap();
    let cqrs = engine(&fixture);
    for id in [HotelId::new(1), HotelId::new(2)] {
        cqrs.execute(
            &id,
            &CmdInitializeHotel { room_count: 2 },
            EventMetadata::default(),
        )
        .await
        .unwrap();
    }
    let game_id = GameId::new("game");
    cqrs.execute(&game_id, &start_game(), EventMetadata::default())
        .await
        .unwrap();
    drop(cqrs);
    let events = PageOnlyStore {
        store: fixture.events.clone(),
        catalog_reads: Default::default(),
    };
    let replacement = read_store::rebuild(&events, &fixture.directory.path().join("paged.db"))
        .await
        .unwrap();
    assert_eq!(
        events
            .catalog_reads
            .load(std::sync::atomic::Ordering::SeqCst),
        8
    );
    for id in [HotelId::new(1), HotelId::new(2)] {
        assert_eq!(
            GetHotelStateQuery::new(id, replacement.clone())
                .apply()
                .await
                .unwrap()
                .rooms
                .len(),
            2
        );
    }
    let game = GetGameQuery::new(game_id, replacement)
        .apply()
        .await
        .unwrap()
        .unwrap();
    verify_game_result(&game, 0, 0, 3, GameStatus::Playing);
}

// A deliberately non-idempotent callback exercises the public runner independently
// of the hotel/game projection definitions. Only the runner prevents double apply.
#[derive(Clone, Default)]
struct CountingProjection {
    fail: bool,
    pause: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl mini_cqrs_es::Projection for CountingProjection {
    type Aggregate = HotelAggregate;

    fn name() -> &'static str {
        "counting"
    }

    async fn apply(
        &self,
        event: &mini_cqrs_es::StoredEvent,
        _payload: HotelEvent,
        executor: &mut dyn toasty::Executor,
    ) -> Result<(), mini_cqrs_es::CqrsError> {
        let previous = HotelReadModel::filter_by_id(&event.aggregate_id)
            .first()
            .exec(&mut *executor)
            .await
            .map_err(read_store::read_error)?;
        let count = previous.map_or(0, |row| row.room_count);
        HotelReadModel::upsert_by_id(&event.aggregate_id)
            .room_count(count + 1)
            .exec(executor)
            .await
            .map_err(read_store::read_error)?;
        if self.fail {
            return Err(mini_cqrs_es::CqrsError::domain_source(ProjectionFailure));
        }
        if let Some(pause) = &self.pause {
            pause.store(true, std::sync::atomic::Ordering::SeqCst);
            std::future::pending::<()>().await;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("injected projection failure")]
struct ProjectionFailure;

fn counting_event(id: u32, version: u64) -> mini_cqrs_es::StoredEvent {
    mini_cqrs_es::StoredEvent {
        id: format!("{id}-{version}"),
        aggregate_id: id.to_string(),
        aggregate_type: HotelAggregate::aggregate_type().into(),
        version,
        event_type: "HotelInitialized".into(),
        schema_version: 1,
        payload: serde_json::to_value(HotelEvent::HotelInitialized { room_count: 1 }).unwrap(),
        metadata: EventMetadata::default(),
        global_sequence: None,
        timestamp: chrono::Utc::now(),
    }
}

async fn count(fixture: &Databases, id: u32) -> u32 {
    HotelReadModel::filter_by_id(id.to_string())
        .first()
        .exec(&mut fixture.reads.clone())
        .await
        .unwrap()
        .map_or(0, |row| row.room_count)
}

#[tokio::test]
async fn public_projection_skips_unrelated_and_duplicate_payloads_and_rejects_gaps() {
    use mini_cqrs_es::{EventConsumer, Projector};
    let fixture = Databases::temporary().await.unwrap();
    let runner = Projector::new(fixture.reads.clone(), CountingProjection::default());
    let id = HotelId::new(1);
    let mut event = counting_event(1, 0);
    event.aggregate_type = "unrelated".into();
    event.payload = serde_json::Value::Null;
    runner.process(&event).await.unwrap(); // Neither version validation nor decoding.
    assert_eq!(runner.last_applied(&id).await.unwrap(), 0);
    for version in [0, 2, u64::MAX] {
        assert!(runner.process(&counting_event(1, version)).await.is_err());
        assert_eq!(runner.last_applied(&id).await.unwrap(), 0);
        assert_eq!(count(&fixture, 1).await, 0);
    }
    runner.process(&counting_event(1, 1)).await.unwrap();
    let mut duplicate = counting_event(1, 1);
    duplicate.payload = serde_json::Value::Null;
    runner.process(&duplicate).await.unwrap(); // Skipped before decoding or callback.
    assert_eq!(count(&fixture, 1).await, 1);
    assert!(runner.process(&counting_event(1, 3)).await.is_err());
    let mut malformed = counting_event(1, 2);
    malformed.payload = serde_json::Value::Null;
    assert!(matches!(
        runner.process(&malformed).await,
        Err(mini_cqrs_es::CqrsError::Serialization(_))
    ));
    assert_eq!(runner.last_applied(&id).await.unwrap(), 1);
    assert_eq!(count(&fixture, 1).await, 1);
    runner.process(&counting_event(1, 2)).await.unwrap();
    runner.process(&counting_event(2, 1)).await.unwrap();
    assert_eq!(runner.last_applied(&id).await.unwrap(), 2);
    assert_eq!(runner.last_applied(&HotelId::new(2)).await.unwrap(), 1);
    assert_eq!(count(&fixture, 1).await, 2);
    // Same stream, a separate projection namespace.
    let hotel = Projector::new(fixture.reads.clone(), HotelProjection);
    assert_eq!(hotel.last_applied(&id).await.unwrap(), 0);
    hotel.process(&counting_event(1, 1)).await.unwrap();
    assert_eq!(hotel.last_applied(&id).await.unwrap(), 1);
    assert_eq!(runner.last_applied(&id).await.unwrap(), 2);
}

#[tokio::test]
async fn callback_error_rolls_back_mutations_and_preserves_source_for_retry() {
    use mini_cqrs_es::{EventConsumer, Projector};
    use std::error::Error;
    let fixture = Databases::temporary().await.unwrap();
    let failed = Projector::new(
        fixture.reads.clone(),
        CountingProjection {
            fail: true,
            pause: None,
        },
    );
    let error = failed.process(&counting_event(1, 1)).await.unwrap_err();
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<ProjectionFailure>()
            .is_some()
    );
    assert_eq!(count(&fixture, 1).await, 0);
    assert_eq!(failed.last_applied(&HotelId::new(1)).await.unwrap(), 0);
    let retry = Projector::new(fixture.reads.clone(), CountingProjection::default());
    retry.process(&counting_event(1, 1)).await.unwrap();
    retry.process(&counting_event(1, 1)).await.unwrap();
    assert_eq!(count(&fixture, 1).await, 1);
    assert_eq!(retry.last_applied(&HotelId::new(1)).await.unwrap(), 1);
}

#[tokio::test]
async fn checkpoint_write_failure_rolls_back_successful_callback() {
    use mini_cqrs_es::{EventConsumer, Projector};
    let fixture = Databases::temporary().await.unwrap();
    let runner = Projector::new(fixture.reads.clone(), CountingProjection::default());
    runner.process(&counting_event(1, 1)).await.unwrap();
    runner.process(&counting_event(2, 1)).await.unwrap();
    runner.process(&counting_event(2, 2)).await.unwrap();
    let mut reads = fixture.reads.clone();
    toasty::sql::query(
        "CREATE UNIQUE INDEX checkpoint_failure ON projection_checkpoints(last_version)",
    )
    .exec(&mut reads)
    .await
    .unwrap();
    // Callback increments stream 1, but checkpoint 1 -> 2 violates the injected index.
    assert!(runner.process(&counting_event(1, 2)).await.is_err());
    assert_eq!(count(&fixture, 1).await, 1);
    assert_eq!(runner.last_applied(&HotelId::new(1)).await.unwrap(), 1);
    // A missing checkpoint insert fails too, after creating a read-model row.
    assert!(runner.process(&counting_event(3, 1)).await.is_err());
    assert_eq!(count(&fixture, 3).await, 0);
    assert_eq!(runner.last_applied(&HotelId::new(3)).await.unwrap(), 0);
    toasty::sql::query("DROP INDEX checkpoint_failure")
        .exec(&mut reads)
        .await
        .unwrap();
    runner.process(&counting_event(1, 2)).await.unwrap();
    runner.process(&counting_event(3, 1)).await.unwrap();
    assert_eq!(count(&fixture, 1).await, 2);
    assert_eq!(count(&fixture, 3).await, 1);
}

#[tokio::test]
async fn cancelling_before_commit_cleans_up_before_a_later_retry() {
    use mini_cqrs_es::{EventConsumer, Projector};
    use std::{
        future::Future,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::Poll,
    };
    let fixture = Databases::temporary().await.unwrap();
    let reached = Arc::new(AtomicBool::new(false));
    let paused = Projector::new(
        fixture.reads.clone(),
        CountingProjection {
            fail: false,
            pause: Some(reached.clone()),
        },
    );
    let event = counting_event(1, 1);
    let mut future = Box::pin(paused.process(&event));
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            if reached.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }),
    )
    .await
    .unwrap();
    drop(future); // Toasty queues rollback; the next use waits for backend cleanup.
    assert_eq!(count(&fixture, 1).await, 0);
    let runner = Projector::new(fixture.reads.clone(), CountingProjection::default());
    assert_eq!(runner.last_applied(&HotelId::new(1)).await.unwrap(), 0);
    runner.process(&event).await.unwrap();
    assert_eq!(count(&fixture, 1).await, 1);
    assert_eq!(runner.last_applied(&HotelId::new(1)).await.unwrap(), 1);
}

#[tokio::test]
async fn corrupt_negative_checkpoint_is_rejected_without_model_changes() {
    use mini_cqrs_es::{EventConsumer, Projector};
    let fixture = Databases::temporary().await.unwrap();
    let runner = Projector::new(fixture.reads.clone(), CountingProjection::default());
    runner.process(&counting_event(1, 1)).await.unwrap();
    toasty::sql::query("UPDATE projection_checkpoints SET last_version = -1")
        .exec(&mut fixture.reads.clone())
        .await
        .unwrap();
    assert!(runner.last_applied(&HotelId::new(1)).await.is_err());
    assert!(runner.process(&counting_event(1, 2)).await.is_err());
    assert_eq!(count(&fixture, 1).await, 1);
}
