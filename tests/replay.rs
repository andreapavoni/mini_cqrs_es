use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use mini_cqrs_es::{
    Aggregate, AggregateManager, CqrsError, EventConsumer, EventConsumers, EventMetadata,
    EventPayload, EventStore, EventStreamReader, LoadedAggregate, NewEvent, SimpleAggregateManager,
    StoredEvent, replay_from, replay_stream,
};
use serde_json::{Value, json};

#[derive(Clone)]
struct History {
    events: Vec<StoredEvent>,
    version: u64,
    fail_load: bool,
    start: u64,
    requests: Arc<Mutex<Vec<(u64, u64, usize)>>>,
    tip_reads: Arc<AtomicUsize>,
    grow_after_read: bool,
    ignore_limit: bool,
    fail_page_after: Option<u64>,
}

impl EventStore for History {
    async fn save_events(
        &self,
        _: &str,
        _: &str,
        _: &[NewEvent],
        _: u64,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        panic!("replay must never write to the event log")
    }

    async fn stream_version(&self, kind: &str, id: &str) -> Result<u64, CqrsError> {
        self.tip_reads.fetch_add(1, Ordering::SeqCst);
        assert_eq!((kind, id), ("Hotel", "42"));
        if self.fail_load {
            return Err(CqrsError::EventStore("load failed".into()));
        }
        Ok(self.version
            + u64::from(self.grow_after_read && !self.requests.lock().unwrap().is_empty()))
    }

    async fn load_events_after(
        &self,
        _: &str,
        _: &str,
        after: u64,
        through: u64,
        limit: std::num::NonZeroUsize,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        self.requests
            .lock()
            .unwrap()
            .push((after, through, limit.get()));
        if self.fail_page_after == Some(after) {
            return Err(CqrsError::EventStore("page read failed".into()));
        }
        let limit = if self.ignore_limit {
            usize::MAX
        } else {
            limit.get()
        };
        Ok(self
            .events
            .iter()
            .skip((after - self.start) as usize)
            .take(limit.min((through - after) as usize))
            .cloned()
            .collect())
    }

    async fn list_streams_after(
        &self,
        _: Option<&mini_cqrs_es::StreamKey>,
        _: std::num::NonZeroUsize,
    ) -> Result<Vec<mini_cqrs_es::StreamKey>, CqrsError> {
        panic!("single-stream replay does not discover streams")
    }

    async fn load_events(&self, _: &str, _: &str) -> Result<(Vec<StoredEvent>, u64), CqrsError> {
        panic!("replay must not use the whole-stream collector")
    }
}

type Deliveries = Arc<Mutex<Vec<(&'static str, Value)>>>;

struct Recorder {
    name: &'static str,
    calls: Deliveries,
    fail_on: Option<u64>,
}

impl EventConsumer for Recorder {
    async fn process(&self, event: &StoredEvent) -> Result<(), CqrsError> {
        self.calls
            .lock()
            .unwrap()
            .push((self.name, serde_json::to_value(event).unwrap()));
        if self.fail_on == Some(event.version) {
            return Err(CqrsError::domain("projection failed"));
        }
        Ok(())
    }
}

fn history(versions: &[u64], tip: u64) -> History {
    History {
        events: versions
            .iter()
            .map(|&version| StoredEvent {
                id: format!("event-{version}"),
                aggregate_type: "Hotel".into(),
                aggregate_id: "42".into(),
                version,
                event_type: "changed".into(),
                schema_version: 1,
                payload: json!({"version": version, "nested": [null, "✓"]}),
                metadata: EventMetadata {
                    actor: Some("historical actor".into()),
                    extra: [("original".into(), json!(true))].into(),
                    ..EventMetadata::default()
                },
                global_sequence: Some(100 + version as i64),
                timestamp: "2026-10-19T10:11:12.123456789Z".parse().unwrap(),
            })
            .collect(),
        version: tip,
        fail_load: false,
        start: 0,
        requests: Arc::default(),
        tip_reads: Arc::default(),
        grow_after_read: false,
        ignore_limit: false,
        fail_page_after: None,
    }
}

fn recorder(name: &'static str, calls: &Deliveries, fail_on: Option<u64>) -> Recorder {
    Recorder {
        name,
        calls: calls.clone(),
        fail_on,
    }
}

#[tokio::test]
async fn delivers_original_envelopes_in_event_and_consumer_order() {
    let store = history(&[1, 2, 3], 3);
    let calls = Deliveries::default();
    let consumers = EventConsumers::new()
        .with(recorder("a", &calls, None))
        .with(recorder("b", &calls, None));
    assert_eq!(
        replay_stream(&store, "Hotel", "42", &consumers)
            .await
            .unwrap(),
        3
    );
    let expected: Vec<_> = store
        .events
        .iter()
        .flat_map(|event| {
            let envelope = serde_json::to_value(event).unwrap();
            [("a", envelope.clone()), ("b", envelope)]
        })
        .collect();
    assert_eq!(*calls.lock().unwrap(), expected);
}

#[tokio::test]
async fn empty_stream_and_load_failure_dispatch_nothing() {
    let calls = Deliveries::default();
    let consumers = EventConsumers::new().with(recorder("a", &calls, None));
    let empty = history(&[], 0);
    assert_eq!(
        replay_stream(&empty, "Hotel", "42", &consumers)
            .await
            .unwrap(),
        0
    );
    let mut broken = history(&[1], 1);
    broken.fail_load = true;
    let error = replay_stream(&broken, "Hotel", "42", &consumers)
        .await
        .unwrap_err();
    assert!(matches!(error, CqrsError::EventStore(message) if message == "load failed"));
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rejects_corrupt_history_before_any_projection_side_effect() {
    let mut wrong_type = history(&[1, 2], 2);
    wrong_type.events[1].aggregate_type = "Game".into();
    let mut wrong_id = history(&[1, 2], 2);
    wrong_id.events[1].aggregate_id = "other".into();
    let invalid = [
        wrong_type,
        wrong_id,
        history(&[0], 1),
        history(&[1, 3], 2),
        history(&[1, 1], 2),
        history(&[2, 1], 2),
        history(&[1, 2], 3),
        history(&[], 1),
    ];
    for store in invalid {
        let calls = Deliveries::default();
        let consumers = EventConsumers::new().with(recorder("a", &calls, None));
        assert!(matches!(
            replay_stream(&store, "Hotel", "42", &consumers).await,
            Err(CqrsError::EventStore(_))
        ));
        assert!(calls.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn stops_on_first_consumer_failure_and_leaves_partial_target() {
    let store = history(&[1, 2, 3], 3);
    let calls = Deliveries::default();
    let consumers = EventConsumers::new()
        .with(recorder("a", &calls, None))
        .with(recorder("b", &calls, Some(2)))
        .with(recorder("c", &calls, None));
    assert!(
        matches!(replay_stream(&store, "Hotel", "42", &consumers).await, Err(CqrsError::Domain(message)) if message == "projection failed")
    );
    let delivered: Vec<_> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(name, event)| (*name, event["version"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        delivered,
        [("a", 1), ("b", 1), ("c", 1), ("a", 2), ("b", 2)]
    );
}

fn page_size(size: usize) -> NonZeroUsize {
    NonZeroUsize::new(size).unwrap()
}

#[tokio::test]
async fn incremental_replay_uses_exact_pages_and_never_reads_after_the_captured_tip() {
    let mut store = history(&[1, 2, 3, 4, 5, 6], 5);
    store.grow_after_read = true;
    let calls = Deliveries::default();
    let consumers = EventConsumers::new().with(recorder("a", &calls, None));
    assert_eq!(
        replay_from(&store, "Hotel", "42", 1, page_size(2), &consumers)
            .await
            .unwrap(),
        5
    );
    assert_eq!(*store.requests.lock().unwrap(), [(1, 5, 2), (3, 5, 2)]);
    assert_eq!(store.tip_reads.load(Ordering::SeqCst), 1);
    let versions: Vec<_> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(_, event)| event["version"].as_u64().unwrap())
        .collect();
    assert_eq!(versions, [2, 3, 4, 5]);
    assert_eq!(
        replay_from(&store, "Hotel", "42", 5, page_size(2), &consumers)
            .await
            .unwrap(),
        6
    );
    assert_eq!(store.tip_reads.load(Ordering::SeqCst), 2);
    assert_eq!(store.requests.lock().unwrap().last(), Some(&(5, 6, 2)));
}

#[tokio::test]
async fn partial_final_page_is_read_and_completed_or_invalid_cursors_read_no_pages() {
    let store = history(&[1, 2, 3], 3);
    let consumers = EventConsumers::new();
    assert_eq!(
        replay_from(&store, "Hotel", "42", 0, page_size(2), &consumers)
            .await
            .unwrap(),
        3
    );
    assert_eq!(*store.requests.lock().unwrap(), [(0, 3, 2), (2, 3, 2)]);
    store.requests.lock().unwrap().clear();
    assert_eq!(
        replay_from(&store, "Hotel", "42", 3, page_size(2), &consumers)
            .await
            .unwrap(),
        3
    );
    assert!(
        replay_from(&store, "Hotel", "42", 4, page_size(2), &consumers)
            .await
            .is_err()
    );
    assert!(store.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn malformed_later_pages_keep_earlier_delivery_but_deliver_none_of_the_bad_page() {
    let mut wrong_type = history(&[1, 2, 3, 4], 4);
    wrong_type.events[3].aggregate_type = "Other".into();
    let mut wrong_id = history(&[1, 2, 3, 4], 4);
    wrong_id.events[3].aggregate_id = "other".into();
    for store in [
        wrong_type,
        wrong_id,
        history(&[1, 2, 2, 4], 4),
        history(&[1, 2, 3, 5], 4),
        history(&[1, 2, 3], 4),
        history(&[1, 2], 4),
    ] {
        let calls = Deliveries::default();
        let consumers = EventConsumers::new().with(recorder("a", &calls, None));
        assert!(
            replay_from(&store, "Hotel", "42", 0, page_size(2), &consumers)
                .await
                .is_err()
        );
        let versions: Vec<_> = calls
            .lock()
            .unwrap()
            .iter()
            .map(|(_, event)| event["version"].as_u64().unwrap())
            .collect();
        assert_eq!(versions, [1, 2]);
    }
    let mut oversized = history(&[1, 2, 3], 3);
    oversized.ignore_limit = true;
    let calls = Deliveries::default();
    let consumers = EventConsumers::new().with(recorder("a", &calls, None));
    let mut reader = EventStreamReader::open(&oversized, "Hotel", "42", 0, page_size(2))
        .await
        .unwrap();
    assert!(reader.next_page().await.is_err());
    assert_eq!(reader.position(), 0);
    assert!(
        replay_from(&oversized, "Hotel", "42", 0, page_size(2), &consumers)
            .await
            .is_err()
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn failed_later_page_propagates_storage_error_after_earlier_delivery() {
    let mut store = history(&[1, 2, 3], 3);
    store.fail_page_after = Some(2);
    let calls = Deliveries::default();
    let consumers = EventConsumers::new().with(recorder("a", &calls, None));
    assert!(
        matches!(replay_from(&store, "Hotel", "42", 0, page_size(2), &consumers).await,
        Err(CqrsError::EventStore(message)) if message == "page read failed")
    );
    assert_eq!(calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn version_cursor_can_complete_at_u64_max_without_overflow() {
    let mut store = history(&[u64::MAX], u64::MAX);
    store.start = u64::MAX - 1;
    let mut reader = EventStreamReader::open(&store, "Hotel", "42", u64::MAX - 1, page_size(2))
        .await
        .unwrap();
    assert_eq!(
        reader.next_page().await.unwrap().unwrap()[0].version,
        u64::MAX
    );
    assert_eq!(reader.position(), u64::MAX);
    assert!(reader.next_page().await.unwrap().is_none());
}

#[derive(Clone, Debug, Default)]
struct Count {
    applied: u64,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Changed {
    version: u64,
}
impl std::fmt::Display for Changed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("changed")
    }
}
impl EventPayload for Changed {
    fn event_type(&self) -> &'static str {
        "changed"
    }
}
impl Aggregate for Count {
    type Id = String;
    type Event = Changed;
    fn aggregate_type() -> &'static str {
        "Hotel"
    }
    async fn apply(&mut self, _: &Changed) {
        self.applied += 1;
    }
}

#[tokio::test]
async fn aggregate_recovery_is_paged_and_does_not_fall_back_to_whole_history() {
    let versions: Vec<_> = (1..=260).collect();
    let store = history(&versions, 260);
    let manager = SimpleAggregateManager::new(store.clone());
    let loaded: LoadedAggregate<Count> = manager.load(&"42".into()).await.unwrap();
    assert_eq!(loaded.version(), 260);
    assert_eq!(loaded.state().applied, 260);
    assert_eq!(
        *store.requests.lock().unwrap(),
        [(0, 260, 256), (256, 260, 256)]
    );
    assert_eq!(store.tip_reads.load(Ordering::SeqCst), 1);
    let mut broken = history(&versions, 260);
    broken.fail_page_after = Some(256);
    let manager = SimpleAggregateManager::new(broken);
    let result: Result<LoadedAggregate<Count>, _> = manager.load(&"42".into()).await;
    assert!(matches!(result, Err(CqrsError::EventStore(message)) if message == "page read failed"));
}
