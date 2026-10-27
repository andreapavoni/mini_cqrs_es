use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use mini_cqrs_es::{
    Aggregate, AggregateManager, Command, Cqrs, CqrsError, EventConsumer, EventConsumers,
    EventMetadata, EventPayload, EventStore, ExecutionError, ExecutionReceipt, LoadedAggregate,
    NewEvent, PostCommitStage, SimpleCqrs, StoredEvent,
};
use serde::{Deserialize, Serialize, Serializer, ser::Error};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fault {
    None,
    Load,
    Handle,
    Serialize,
    Conflict,
    BeforeWrite,
    AfterWrite,
    Apply,
    Consumer,
    Snapshot,
    WrongStream,
    Gap,
}

#[derive(Clone, Deserialize)]
enum Change {
    Increment,
    Unserializable,
    RefuseApply,
}

impl Serialize for Change {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Increment => serializer.serialize_str("Increment"),
            Self::RefuseApply => serializer.serialize_str("RefuseApply"),
            Self::Unserializable => Err(S::Error::custom("event cannot be serialized")),
        }
    }
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("change")
    }
}

impl EventPayload for Change {
    fn event_type(&self) -> &'static str {
        "change"
    }
}

#[derive(Clone, Debug, Default)]
struct Counter {
    count: u64,
    apply_calls: Arc<AtomicUsize>,
}

impl Aggregate for Counter {
    type Id = u64;
    type Event = Change;

    fn aggregate_type() -> &'static str {
        "test.counter"
    }

    async fn apply(&mut self, _: &Change) {
        self.apply_calls.fetch_add(1, Ordering::SeqCst);
        self.count += 1;
    }

    async fn apply_events(&mut self, events: &[StoredEvent]) -> Result<(), CqrsError> {
        for event in events {
            let change = event.get_payload::<Change>()?;
            if matches!(change, Change::RefuseApply) {
                return Err(CqrsError::new("application failed"));
            }
            self.apply(&change).await;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Probe {
    handle_calls: usize,
    append_calls: usize,
    store_calls: usize,
    persisted: Vec<StoredEvent>,
    delivered: Vec<StoredEvent>,
    aggregate: Option<Value>,
    apply_calls: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct Fixture {
    fault: Fault,
    probe: Arc<Mutex<Probe>>,
}

impl AggregateManager<Counter> for Fixture {
    async fn load(&self, id: &u64) -> Result<LoadedAggregate<Counter>, CqrsError> {
        if self.fault == Fault::Load {
            return Err(CqrsError::EventStore("load failed".into()));
        }
        let state = Counter {
            count: 0,
            apply_calls: self.probe.lock().unwrap().apply_calls.clone(),
        };
        Ok(LoadedAggregate::new(*id, 7, state))
    }

    async fn store(&self, aggregate: &LoadedAggregate<Counter>) -> Result<(), CqrsError> {
        let mut probe = self.probe.lock().unwrap();
        probe.store_calls += 1;
        if self.fault == Fault::Snapshot {
            return Err(CqrsError::SnapshotStore("snapshot failed".into()));
        }
        probe.aggregate = Some(json!({
            "id": aggregate.id(), "version": aggregate.version(), "count": aggregate.state().count,
        }));
        Ok(())
    }
}

impl EventStore for Fixture {
    async fn stream_version(&self, _: &str, _: &str) -> Result<u64, CqrsError> {
        unreachable!("the fixture aggregate manager supplies the initial state")
    }

    async fn load_events_after(
        &self,
        _: &str,
        _: &str,
        _: u64,
        _: u64,
        _: std::num::NonZeroUsize,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        unreachable!("the fixture aggregate manager supplies the initial state")
    }

    async fn list_streams_after(
        &self,
        _: Option<&mini_cqrs_es::StreamKey>,
        _: std::num::NonZeroUsize,
    ) -> Result<Vec<mini_cqrs_es::StreamKey>, CqrsError> {
        unreachable!("command execution does not discover streams")
    }

    async fn save_events(
        &self,
        kind: &str,
        id: &str,
        events: &[NewEvent],
        expected_version: u64,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        let mut probe = self.probe.lock().unwrap();
        probe.append_calls += 1;
        assert_eq!(expected_version, 7);
        if self.fault == Fault::Conflict {
            return Err(CqrsError::Conflict {
                expected_version,
                actual_version: 8,
            });
        }
        if self.fault == Fault::BeforeWrite {
            return Err(CqrsError::EventStore("append failed".into()));
        }
        let mut events: Vec<_> = events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                let version = expected_version + index as u64 + 1;
                StoredEvent {
                    id: format!("event-{version}"),
                    aggregate_type: kind.into(),
                    aggregate_id: id.into(),
                    version,
                    event_type: event.event_type.clone(),
                    schema_version: event.schema_version,
                    payload: event.payload.clone(),
                    metadata: event.metadata.clone(),
                    timestamp: event.timestamp,
                    global_sequence: None,
                }
            })
            .collect();
        match self.fault {
            Fault::WrongStream => events[1].aggregate_id = "another-stream".into(),
            Fault::Gap => events[1].version = 10,
            _ => {}
        }
        probe.persisted.extend(events.clone());
        if self.fault == Fault::AfterWrite {
            return Err(CqrsError::EventStore("append failed".into()));
        }
        Ok(events)
    }
}

#[tokio::test]
async fn invalid_committed_batches_are_rejected_before_any_domain_application() {
    for fault in [Fault::WrongStream, Fault::Gap] {
        let (engine, command, probe) = scenario(fault);
        let error = engine.execute(&42, &command, metadata()).await.unwrap_err();
        let ExecutionError::Committed {
            receipt,
            stage,
            source,
        } = error
        else {
            panic!("{error:?}")
        };
        assert_eq!(stage, PostCommitStage::AggregateApply);
        assert_eq!(receipt.event_ids, vec!["event-8", "event-9"]);
        assert!(matches!(source, CqrsError::EventStore(_)));
        let probe = probe.lock().unwrap();
        assert_eq!(probe.persisted.len(), 2);
        assert_eq!(probe.apply_calls.load(Ordering::SeqCst), 0);
        assert!(probe.delivered.is_empty());
        assert_eq!(probe.store_calls, 0);
    }
}

impl EventConsumer for Fixture {
    async fn process(&self, event: &StoredEvent) -> Result<(), CqrsError> {
        if self.fault == Fault::Consumer && event.version == 9 {
            return Err(CqrsError::new("consumer failed"));
        }
        self.probe.lock().unwrap().delivered.push(event.clone());
        Ok(())
    }
}

struct Emit {
    fixture: Fixture,
    events: Vec<Change>,
}

impl Command for Emit {
    type Aggregate = Counter;

    async fn handle(&self, _: &Counter) -> Result<Vec<Change>, CqrsError> {
        self.fixture.probe.lock().unwrap().handle_calls += 1;
        if self.fixture.fault == Fault::Handle {
            return Err(CqrsError::domain_source(std::io::Error::other(
                "command rejected",
            )));
        }
        Ok(self.events.clone())
    }
}

fn scenario(fault: Fault) -> (SimpleCqrs<Fixture, Fixture>, Emit, Arc<Mutex<Probe>>) {
    let probe = Arc::new(Mutex::new(Probe::default()));
    let fixture = Fixture {
        fault,
        probe: probe.clone(),
    };
    let events = match fault {
        Fault::Serialize => vec![Change::Increment, Change::Unserializable],
        Fault::Apply => vec![Change::RefuseApply, Change::Increment],
        _ => vec![Change::Increment, Change::Increment],
    };
    let engine = SimpleCqrs::from_parts(
        fixture.clone(),
        fixture.clone(),
        EventConsumers::new().with(fixture.clone()),
    );
    (engine, Emit { fixture, events }, probe)
}

fn metadata() -> EventMetadata {
    EventMetadata {
        command_id: Some("command-1".into()),
        correlation_id: Some("request-1".into()),
        causation_id: Some("parent-event".into()),
        actor: Some("alice".into()),
        tenant_id: Some("tenant-1".into()),
        extra: [("context".into(), json!({"nested": [1, null, "caffè"]}))].into(),
    }
}

fn receipt() -> ExecutionReceipt<u64> {
    ExecutionReceipt {
        aggregate_id: 42,
        version: 9,
        event_ids: vec!["event-8".into(), "event-9".into()],
    }
}

#[tokio::test]
async fn metadata_reaches_every_persisted_and_delivered_event_with_an_ordered_receipt() {
    let (engine, command, probe) = scenario(Fault::None);
    assert_eq!(
        engine.execute(&42, &command, metadata()).await.unwrap(),
        receipt()
    );
    let probe = probe.lock().unwrap();
    assert_eq!(probe.persisted.len(), 2);
    assert_eq!(probe.delivered.len(), 2);
    for event in probe.persisted.iter().chain(&probe.delivered) {
        assert_eq!(
            serde_json::to_value(&event.metadata).unwrap(),
            serde_json::to_value(metadata()).unwrap()
        );
    }
    assert_eq!(
        probe.aggregate,
        Some(json!({"id":42, "version":9, "count":2}))
    );
    assert_eq!(probe.append_calls, 1);
}

#[tokio::test]
async fn empty_command_checks_append_and_retains_loaded_tip_without_inventing_events() {
    let (engine, mut command, probe) = scenario(Fault::None);
    command.events.clear();
    assert_eq!(
        engine.execute(&42, &command, metadata()).await.unwrap(),
        ExecutionReceipt {
            aggregate_id: 42,
            version: 7,
            event_ids: vec![],
        }
    );
    let probe = probe.lock().unwrap();
    assert_eq!(probe.append_calls, 1);
    assert!(probe.persisted.is_empty());
    assert!(probe.delivered.is_empty());
    assert_eq!(
        probe.aggregate,
        Some(json!({"id":42, "version":7, "count":0}))
    );
}

#[tokio::test]
async fn pre_append_failures_and_conflicts_are_not_committed() {
    for fault in [
        Fault::Load,
        Fault::Handle,
        Fault::Serialize,
        Fault::Conflict,
    ] {
        let (engine, command, probe) = scenario(fault);
        let error = engine.execute(&42, &command, metadata()).await.unwrap_err();
        let ExecutionError::NotCommitted { source } = error else {
            panic!("{fault:?}: {error:?}")
        };
        match fault {
            Fault::Load => assert!(matches!(source, CqrsError::EventStore(_))),
            Fault::Handle => assert!(matches!(source, CqrsError::DomainSource(_))),
            Fault::Serialize => assert!(matches!(source, CqrsError::Serialization(_))),
            Fault::Conflict => assert!(matches!(
                source,
                CqrsError::Conflict {
                    expected_version: 7,
                    actual_version: 8
                }
            )),
            _ => unreachable!(),
        }
        let probe = probe.lock().unwrap();
        assert_eq!(probe.append_calls, usize::from(fault == Fault::Conflict));
        assert_eq!(probe.handle_calls, usize::from(fault != Fault::Load));
        assert!(probe.persisted.is_empty());
        assert!(probe.delivered.is_empty());
        assert_eq!(probe.store_calls, 0);
    }
}

#[tokio::test]
async fn append_errors_are_uncertain_even_when_events_were_written_and_are_not_retried() {
    for fault in [Fault::BeforeWrite, Fault::AfterWrite] {
        let (engine, command, probe) = scenario(fault);
        let error = engine.execute(&42, &command, metadata()).await.unwrap_err();
        assert!(
            matches!(error, ExecutionError::CommitUnknown { source: CqrsError::EventStore(message) } if message == "append failed")
        );
        let probe = probe.lock().unwrap();
        assert_eq!(probe.append_calls, 1);
        assert_eq!(
            probe.persisted.len(),
            if fault == Fault::AfterWrite { 2 } else { 0 }
        );
        assert!(probe.delivered.is_empty());
        assert_eq!(probe.store_calls, 0);
    }
}

#[tokio::test]
async fn all_post_commit_failures_retain_the_full_receipt_and_original_source() {
    for (fault, expected_stage, message) in [
        (
            Fault::Apply,
            PostCommitStage::AggregateApply,
            "application failed",
        ),
        (
            Fault::Consumer,
            PostCommitStage::Consumers,
            "consumer failed",
        ),
        (
            Fault::Snapshot,
            PostCommitStage::AggregateStore,
            "snapshot store error: snapshot failed",
        ),
    ] {
        let (engine, command, probe) = scenario(fault);
        let error = engine.execute(&42, &command, metadata()).await.unwrap_err();
        assert_eq!(
            std::error::Error::source(&error).unwrap().to_string(),
            message
        );
        let ExecutionError::Committed {
            receipt: actual,
            stage,
            source,
        } = error
        else {
            panic!("{error:?}")
        };
        assert_eq!(actual, receipt());
        assert_eq!(stage, expected_stage);
        assert_eq!(source.to_string(), message);
        let probe = probe.lock().unwrap();
        assert_eq!(probe.persisted.len(), 2);
        assert_eq!(
            probe.delivered.len(),
            match fault {
                Fault::Apply => 0,
                Fault::Consumer => 1,
                _ => 2,
            }
        );
        assert_eq!(probe.store_calls, usize::from(fault == Fault::Snapshot));
    }
}

async fn through_trait<E: Cqrs<Counter>>(
    engine: &E,
    command: &Emit,
    metadata: EventMetadata,
) -> Result<ExecutionReceipt<u64>, ExecutionError<u64>> {
    fn assert_send<T: Send>(_: &T) {}
    let future = engine.execute(&42, command, metadata);
    assert_send(&future);
    future.await
}

#[tokio::test]
async fn generic_cqrs_execution_preserves_metadata_receipts_and_error_sources() {
    let (engine, command, probe) = scenario(Fault::None);
    assert_eq!(
        through_trait(&engine, &command, metadata()).await.unwrap(),
        receipt()
    );
    for event in &probe.lock().unwrap().persisted {
        assert_eq!(
            serde_json::to_value(&event.metadata).unwrap(),
            serde_json::to_value(metadata()).unwrap()
        );
    }
    for fault in [Fault::Handle, Fault::AfterWrite, Fault::Consumer] {
        let (engine, command, _) = scenario(fault);
        let error = through_trait(&engine, &command, metadata())
            .await
            .unwrap_err();
        let source = std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<CqrsError>()
            .unwrap();
        match fault {
            Fault::Handle => {
                assert!(matches!(&error, ExecutionError::NotCommitted { .. }));
                assert!(matches!(source, CqrsError::DomainSource(_)));
                assert_eq!(
                    std::error::Error::source(source).unwrap().to_string(),
                    "command rejected"
                );
            }
            Fault::AfterWrite => {
                assert!(matches!(&error, ExecutionError::CommitUnknown { .. }));
                assert!(
                    matches!(source, CqrsError::EventStore(message) if message == "append failed")
                );
            }
            Fault::Consumer => {
                let ExecutionError::Committed {
                    receipt: actual,
                    stage,
                    ..
                } = &error
                else {
                    panic!("{error:?}")
                };
                assert_eq!(actual, &receipt());
                assert_eq!(*stage, PostCommitStage::Consumers);
                assert!(
                    matches!(source, CqrsError::Domain(message) if message == "consumer failed")
                );
            }
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn default_metadata_is_preserved_without_generating_context_ids() {
    let (engine, command, probe) = scenario(Fault::None);
    assert_eq!(
        through_trait(&engine, &command, EventMetadata::default())
            .await
            .unwrap(),
        receipt()
    );
    for event in &probe.lock().unwrap().persisted {
        assert_eq!(
            serde_json::to_value(&event.metadata).unwrap(),
            serde_json::to_value(EventMetadata::default()).unwrap()
        );
    }
}
