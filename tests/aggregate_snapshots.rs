use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use mini_cqrs_es::{
    Aggregate, AggregateManager, AggregateSnapshot, Command, Cqrs, CqrsError, DEFAULT_PAGE_SIZE,
    EventConsumers, EventMetadata, EventPayload, EventStore, ExecutionError, LoadedAggregate,
    NewEvent, PostCommitStage, SimpleCqrs, SnapshotAggregateManager, SnapshotStore, StoredEvent,
    StreamKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Balance {
    amount: u64,
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

impl Aggregate for Balance {
    type Id = u64;
    type Event = Credit;

    fn aggregate_type() -> &'static str {
        "test.balance"
    }
    async fn apply(&mut self, event: &Credit) {
        self.amount += event.0;
    }
}

type SnapshotRows = HashMap<(String, String), (String, u64, Value)>;

#[derive(Clone, Default)]
struct Snapshots {
    rows: Arc<Mutex<SnapshotRows>>,
    fail_load: bool,
    fail_save: bool,
    append_on_load: Option<History>,
}

impl SnapshotStore for Snapshots {
    async fn save_snapshot<A: Aggregate>(
        &self,
        snapshot: AggregateSnapshot<A>,
    ) -> Result<(), CqrsError> {
        if self.fail_save {
            return Err(CqrsError::Other(SnapshotFailure("save").into()));
        }
        self.rows.lock().unwrap().insert(
            (
                A::aggregate_type().into(),
                snapshot.aggregate_id.to_string(),
            ),
            (
                snapshot.aggregate_id.to_string(),
                snapshot.version,
                snapshot.payload().clone(),
            ),
        );
        Ok(())
    }

    async fn load_snapshot<A: Aggregate>(
        &self,
        id: &A::Id,
    ) -> Result<Option<AggregateSnapshot<A>>, CqrsError> {
        if self.fail_load {
            return Err(CqrsError::Other(SnapshotFailure("load").into()));
        }
        // Models a new event/snapshot becoming visible during the snapshot read.
        if let Some(history) = &self.append_on_load {
            let mut rows = history.rows.lock().unwrap();
            let next = rows.len() as u64 + 1;
            rows.push(credit_event(next));
        }
        let snapshots = self.rows.lock().unwrap();
        let Some((stored_id, version, payload)) =
            snapshots.get(&(A::aggregate_type().into(), id.to_string()))
        else {
            return Ok(None);
        };
        if stored_id != &id.to_string() {
            return Err(CqrsError::SnapshotStore("wrong stored identity".into()));
        }
        Ok(Some(AggregateSnapshot::from_parts(
            id.clone(),
            *version,
            payload.clone(),
        )))
    }
}

#[test]
fn snapshot_payload_contains_only_domain_state_and_uses_exact_runtime_metadata() {
    for version in [0, 7] {
        let loaded = LoadedAggregate::new(42, version, Balance { amount: 100 });
        let snapshot = AggregateSnapshot::new(&loaded).unwrap();
        assert_eq!(snapshot.aggregate_id, 42);
        assert_eq!(snapshot.version, version);
        assert_eq!(snapshot.payload(), &json!({"amount":100}));
        assert_eq!(snapshot.get_payload().unwrap(), *loaded.state());
    }
}

#[tokio::test]
async fn snapshot_manager_round_trips_wrapper_metadata_and_state() {
    let snapshots = Snapshots::default();
    let history = History::credits(7);
    let manager = SnapshotAggregateManager::new(history.clone(), snapshots.clone());
    let original = LoadedAggregate::new(42, 7, Balance { amount: 7 });
    manager.store(&original).await.unwrap();
    let restored: LoadedAggregate<Balance> = manager.load(&42).await.unwrap();
    assert_eq!(restored.id(), &42);
    assert_eq!(restored.version(), 7);
    assert_eq!(restored.state(), original.state());
    // The store handles raw JSON envelopes without a Serde bound on its generic A.
    let saved = snapshots
        .load_snapshot::<Balance>(&42)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.payload(), &json!({"amount":7}));
    assert!(history.requests.lock().unwrap().is_empty());
    assert_eq!(history.tip_reads.load(Ordering::SeqCst), 1);
}

#[test]
fn malformed_snapshot_payload_propagates_a_decoding_error() {
    let snapshot = AggregateSnapshot::<Balance>::from_parts(42, 7, json!({"amount":"invalid"}));
    assert!(matches!(
        snapshot.get_payload(),
        Err(CqrsError::Serialization(_))
    ));
}

#[derive(Debug, thiserror::Error)]
#[error("snapshot {0} failed")]
struct SnapshotFailure(&'static str);

type PageRequests = Arc<Mutex<Vec<(u64, u64, usize)>>>;

#[derive(Clone, Default)]
struct History {
    rows: Arc<Mutex<Vec<StoredEvent>>>,
    requests: PageRequests,
    tip_reads: Arc<AtomicUsize>,
    fail_tip: bool,
    fail_page_after: Option<u64>,
    append_during_read: bool,
}

impl History {
    fn credits(count: u64) -> Self {
        Self {
            rows: Arc::new(Mutex::new((1..=count).map(credit_event).collect())),
            ..Self::default()
        }
    }
}

fn credit_event(version: u64) -> StoredEvent {
    let event = NewEvent::from_payload(Credit(1), EventMetadata::default()).unwrap();
    StoredEvent {
        id: format!("event-{version}"),
        aggregate_id: "42".into(),
        aggregate_type: Balance::aggregate_type().into(),
        version,
        event_type: event.event_type,
        schema_version: event.schema_version,
        payload: event.payload,
        metadata: event.metadata,
        global_sequence: None,
        timestamp: event.timestamp,
    }
}

impl EventStore for History {
    async fn stream_version(&self, kind: &str, id: &str) -> Result<u64, CqrsError> {
        assert_eq!((kind, id), (Balance::aggregate_type(), "42"));
        self.tip_reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_tip {
            return Err(CqrsError::EventStore("tip unavailable".into()));
        }
        Ok(self.rows.lock().unwrap().len() as u64)
    }

    async fn load_events_after(
        &self,
        kind: &str,
        id: &str,
        after: u64,
        through: u64,
        limit: NonZeroUsize,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        assert_eq!((kind, id), (Balance::aggregate_type(), "42"));
        self.requests
            .lock()
            .unwrap()
            .push((after, through, limit.get()));
        if self.fail_page_after == Some(after) {
            return Err(CqrsError::EventStore("tail unavailable".into()));
        }
        let mut rows = self.rows.lock().unwrap();
        if self.append_during_read {
            let next = rows.len() as u64 + 1;
            rows.push(credit_event(next));
        }
        Ok(rows
            .iter()
            .skip(after as usize)
            .take(limit.get().min((through - after) as usize))
            .cloned()
            .collect())
    }

    async fn save_events(
        &self,
        kind: &str,
        id: &str,
        events: &[NewEvent],
        expected: u64,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        assert_eq!((kind, id), (Balance::aggregate_type(), "42"));
        let mut rows = self.rows.lock().unwrap();
        if rows.len() as u64 != expected {
            return Err(CqrsError::Conflict {
                expected_version: expected,
                actual_version: rows.len() as u64,
            });
        }
        let stored: Vec<_> = events
            .iter()
            .enumerate()
            .map(|(offset, event)| {
                let mut stored = credit_event(expected + offset as u64 + 1);
                stored.event_type = event.event_type.clone();
                stored.payload = event.payload.clone();
                stored.metadata = event.metadata.clone();
                stored.timestamp = event.timestamp;
                stored
            })
            .collect();
        rows.extend(stored.clone());
        Ok(stored)
    }

    async fn list_streams_after(
        &self,
        _: Option<&StreamKey>,
        _: NonZeroUsize,
    ) -> Result<Vec<StreamKey>, CqrsError> {
        panic!("aggregate recovery must not discover streams")
    }

    async fn load_events(&self, _: &str, _: &str) -> Result<(Vec<StoredEvent>, u64), CqrsError> {
        panic!("aggregate recovery must not collect the whole history")
    }
}

async fn snapshots_at(version: u64) -> Snapshots {
    let snapshots = Snapshots::default();
    snapshots
        .save_snapshot(
            AggregateSnapshot::new(&LoadedAggregate::new(
                42,
                version,
                Balance { amount: version },
            ))
            .unwrap(),
        )
        .await
        .unwrap();
    snapshots
}

#[tokio::test]
async fn absent_snapshot_recovers_full_history_or_a_new_aggregate() {
    for tip in [0, 3] {
        let snapshots = Snapshots::default();
        assert!(
            snapshots
                .load_snapshot::<Balance>(&42)
                .await
                .unwrap()
                .is_none()
        );
        let history = History::credits(tip);
        let manager = SnapshotAggregateManager::new(history.clone(), snapshots.clone());
        let loaded: LoadedAggregate<Balance> = manager.load(&42).await.unwrap();
        assert_eq!(loaded.id(), &42);
        assert_eq!(loaded.version(), tip);
        assert_eq!(loaded.state().amount, tip);
        // Loading does not implicitly write a replacement snapshot.
        assert!(snapshots.rows.lock().unwrap().is_empty());
        assert_eq!(history.tip_reads.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn stale_snapshot_replays_only_bounded_tail_through_the_captured_tip() {
    let prefix = 17;
    let tip = prefix + DEFAULT_PAGE_SIZE.get() as u64 * 2 + 5;
    let mut history = History::credits(tip);
    history.append_during_read = true;
    // A trusted snapshot replaces the prefix: those payloads are not decoded.
    for event in history
        .rows
        .lock()
        .unwrap()
        .iter_mut()
        .take(prefix as usize)
    {
        event.payload = Value::Null;
    }
    let snapshots = snapshots_at(prefix).await;
    let manager = SnapshotAggregateManager::new(history.clone(), snapshots.clone());
    let loaded: LoadedAggregate<Balance> = manager.load(&42).await.unwrap();
    assert_eq!(loaded.version(), tip);
    assert_eq!(loaded.state().amount, tip);
    assert_eq!(
        *history.requests.lock().unwrap(),
        vec![
            (prefix, tip, 256),
            (prefix + 256, tip, 256),
            (prefix + 512, tip, 256)
        ]
    );
    assert_eq!(history.tip_reads.load(Ordering::SeqCst), 1);
    assert_eq!(history.rows.lock().unwrap().len() as u64, tip + 3);
    assert_eq!(
        snapshots
            .load_snapshot::<Balance>(&42)
            .await
            .unwrap()
            .unwrap()
            .version,
        prefix
    );
}

#[tokio::test]
async fn zero_version_snapshot_is_a_trusted_initial_state_cache() {
    let manager = SnapshotAggregateManager::new(History::credits(2), snapshots_at(0).await);
    let loaded: LoadedAggregate<Balance> = manager.load(&42).await.unwrap();
    assert_eq!(loaded.version(), 2);
    assert_eq!(loaded.state().amount, 2);
}

#[tokio::test]
async fn snapshot_storage_and_decoding_failures_are_not_absence() {
    let history = History::credits(3);
    let snapshots = Snapshots {
        fail_load: true,
        ..Snapshots::default()
    };
    let manager = SnapshotAggregateManager::new(history.clone(), snapshots);
    let result: Result<LoadedAggregate<Balance>, _> = manager.load(&42).await;
    let CqrsError::Other(source) = result.unwrap_err() else {
        panic!("source was lost")
    };
    assert_eq!(source.downcast_ref::<SnapshotFailure>().unwrap().0, "load");
    assert_eq!(history.tip_reads.load(Ordering::SeqCst), 0);

    let snapshots = snapshots_at(2).await;
    snapshots
        .rows
        .lock()
        .unwrap()
        .get_mut(&(Balance::aggregate_type().into(), "42".into()))
        .unwrap()
        .2 = json!({"amount":"invalid"});
    let manager = SnapshotAggregateManager::new(history.clone(), snapshots);
    let result: Result<LoadedAggregate<Balance>, _> = manager.load(&42).await;
    assert!(matches!(result, Err(CqrsError::Serialization(_))));
    assert_eq!(history.tip_reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn wrong_identity_and_snapshots_ahead_of_history_are_errors() {
    let history = History::credits(2);
    let snapshots = snapshots_at(2).await;
    snapshots
        .rows
        .lock()
        .unwrap()
        .get_mut(&(Balance::aggregate_type().into(), "42".into()))
        .unwrap()
        .0 = "99".into();
    let manager = SnapshotAggregateManager::new(history.clone(), snapshots);
    let result: Result<LoadedAggregate<Balance>, _> = manager.load(&42).await;
    assert!(matches!(result, Err(CqrsError::SnapshotStore(_))));
    assert_eq!(history.tip_reads.load(Ordering::SeqCst), 0);
    for tip in [0, 2] {
        let manager = SnapshotAggregateManager::new(History::credits(tip), snapshots_at(3).await);
        let result: Result<LoadedAggregate<Balance>, _> = manager.load(&42).await;
        assert!(matches!(result, Err(CqrsError::EventStore(_))));
    }
}

#[tokio::test]
async fn event_tip_and_tail_failures_propagate_without_returning_stale_state() {
    for fail_tip in [true, false] {
        let mut history = History::credits(3);
        history.fail_tip = fail_tip;
        history.fail_page_after = Some(1);
        let manager = SnapshotAggregateManager::new(history, snapshots_at(1).await);
        let result: Result<LoadedAggregate<Balance>, _> = manager.load(&42).await;
        assert!(matches!(result, Err(CqrsError::EventStore(_))));
    }
    for fault in ["gap", "wrong type", "wrong id", "invalid payload"] {
        let history = History::credits(3);
        {
            let mut rows = history.rows.lock().unwrap();
            match fault {
                "gap" => rows[1].version = 3,
                "wrong type" => rows[1].aggregate_type = "unrelated".into(),
                "wrong id" => rows[1].aggregate_id = "99".into(),
                _ => rows[1].payload = Value::Null,
            }
        }
        let manager = SnapshotAggregateManager::new(history, snapshots_at(1).await);
        let result: Result<LoadedAggregate<Balance>, _> = manager.load(&42).await;
        assert!(result.is_err(), "{fault} must not return a stale snapshot");
    }
}

struct AddCredit;
impl Command for AddCredit {
    type Aggregate = Balance;
    async fn handle(&self, state: &Balance) -> Result<Vec<Credit>, CqrsError> {
        // The command must see the recovered tail, not just the old snapshot.
        assert_eq!(state.amount, 3);
        Ok(vec![Credit(10)])
    }
}

#[tokio::test]
async fn failed_snapshot_save_after_append_recovers_from_the_older_snapshot() {
    let history = History::credits(3);
    let snapshots = snapshots_at(1).await;
    let mut failing = snapshots.clone();
    failing.fail_save = true;
    let cqrs = SimpleCqrs::from_parts(
        SnapshotAggregateManager::new(history.clone(), failing),
        history.clone(),
        EventConsumers::new(),
    );
    let error = cqrs
        .execute(&42, &AddCredit, EventMetadata::default())
        .await
        .unwrap_err();
    let ExecutionError::Committed {
        receipt,
        stage: PostCommitStage::AggregateStore,
        source: CqrsError::Other(source),
    } = error
    else {
        panic!("snapshot failure must retain confirmed commit and source")
    };
    assert_eq!(receipt.version, 4);
    assert_eq!(source.downcast_ref::<SnapshotFailure>().unwrap().0, "save");
    assert_eq!(
        snapshots
            .load_snapshot::<Balance>(&42)
            .await
            .unwrap()
            .unwrap()
            .version,
        1
    );
    let manager = SnapshotAggregateManager::new(history, snapshots.clone());
    let recovered: LoadedAggregate<Balance> = manager.load(&42).await.unwrap();
    assert_eq!(recovered.version(), 4);
    assert_eq!(recovered.state().amount, 13);
    // Saving the repaired cache is explicit and does not execute the command again.
    manager.store(&recovered).await.unwrap();
    assert_eq!(
        snapshots
            .load_snapshot::<Balance>(&42)
            .await
            .unwrap()
            .unwrap()
            .version,
        4
    );
}

#[tokio::test]
async fn snapshot_is_loaded_before_capturing_tip_to_allow_concurrent_snapshots() {
    let history = History::credits(1);
    let mut snapshots = snapshots_at(2).await;
    snapshots.append_on_load = Some(history.clone());
    let manager = SnapshotAggregateManager::new(history.clone(), snapshots);
    let loaded: LoadedAggregate<Balance> = manager.load(&42).await.unwrap();
    assert_eq!(loaded.version(), 2);
    assert_eq!(loaded.state().amount, 2);
    assert!(history.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn snapshot_load_failure_prevents_command_execution_and_append() {
    let history = History::credits(3);
    let snapshots = Snapshots {
        fail_load: true,
        ..Snapshots::default()
    };
    let cqrs = SimpleCqrs::from_parts(
        SnapshotAggregateManager::new(history.clone(), snapshots),
        history.clone(),
        EventConsumers::new(),
    );
    let error = cqrs
        .execute(&42, &AddCredit, EventMetadata::default())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ExecutionError::NotCommitted {
            source: CqrsError::Other(_)
        }
    ));
    assert_eq!(history.rows.lock().unwrap().len(), 3);
    assert_eq!(history.tip_reads.load(Ordering::SeqCst), 0);
}
