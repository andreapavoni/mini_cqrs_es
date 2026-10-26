use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use mini_cqrs_es::{
    Aggregate, AggregateManager, Command, Cqrs, CqrsError, EventConsumers, EventMetadata,
    EventPayload, EventStore, ExecutionError, LoadedAggregate, NewEvent, SimpleAggregateManager,
    SimpleCqrs, StoredEvent,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
struct Account {
    opened: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum AccountEvent {
    Opened,
}

impl std::fmt::Display for AccountEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("account_opened")
    }
}

impl EventPayload for AccountEvent {
    fn event_type(&self) -> &'static str {
        "account_opened"
    }
}

impl Aggregate for Account {
    type Id = String;
    type Event = AccountEvent;

    fn aggregate_type() -> &'static str {
        "test.account"
    }

    async fn apply(&mut self, _: &AccountEvent) {
        self.opened = true;
    }
}

struct OpenAccount(Arc<AtomicUsize>);

impl Command for OpenAccount {
    type Aggregate = Account;

    async fn handle(&self, account: &Account) -> Result<Vec<AccountEvent>, CqrsError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        assert!(!account.opened);
        Ok(vec![AccountEvent::Opened])
    }
}

#[derive(Clone)]
struct ProbeStore {
    fail_load: bool,
    appends: Arc<AtomicUsize>,
}

impl EventStore for ProbeStore {
    async fn stream_version(&self, _: &str, _: &str) -> Result<u64, CqrsError> {
        if self.fail_load {
            Err(CqrsError::EventStore("unavailable event history".into()))
        } else {
            Ok(0)
        }
    }

    async fn load_events_after(
        &self,
        _: &str,
        _: &str,
        _: u64,
        _: u64,
        _: std::num::NonZeroUsize,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        unreachable!("empty stream requires no event page")
    }

    async fn list_streams_after(
        &self,
        _: Option<&mini_cqrs_es::StreamKey>,
        _: std::num::NonZeroUsize,
    ) -> Result<Vec<mini_cqrs_es::StreamKey>, CqrsError> {
        unreachable!("aggregate loading does not discover streams")
    }

    async fn save_events(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        events: &[NewEvent],
        expected_version: u64,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        self.appends.fetch_add(1, Ordering::SeqCst);
        assert_eq!(expected_version, 0);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        Ok(vec![StoredEvent {
            id: "event-1".into(),
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            version: 1,
            event_type: event.event_type.clone(),
            schema_version: event.schema_version,
            payload: event.payload.clone(),
            metadata: event.metadata.clone(),
            timestamp: event.timestamp,
            global_sequence: None,
        }])
    }
}

#[tokio::test]
async fn load_failure_prevents_command_handling_and_append() {
    let store = ProbeStore {
        fail_load: true,
        appends: Arc::default(),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let cqrs = SimpleCqrs::from_parts(
        SimpleAggregateManager::new(store.clone()),
        store.clone(),
        EventConsumers::new(),
    );

    let result = cqrs
        .execute(
            &"account-1".into(),
            &OpenAccount(calls.clone()),
            EventMetadata::default(),
        )
        .await;

    assert!(
        matches!(result, Err(ExecutionError::NotCommitted { source: CqrsError::EventStore(message) }) if message == "unavailable event history")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(store.appends.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn successful_empty_load_allows_new_aggregate_creation() {
    let store = ProbeStore {
        fail_load: false,
        appends: Arc::default(),
    };
    let manager = SimpleAggregateManager::new(store.clone());
    let id = "account-1".to_owned();
    let empty: LoadedAggregate<Account> = manager.load(&id).await.unwrap();
    assert_eq!(empty.id(), &id);
    assert_eq!(empty.version(), 0);
    assert!(!empty.state().opened);

    let calls = Arc::new(AtomicUsize::new(0));
    let cqrs = SimpleCqrs::from_parts(manager, store.clone(), EventConsumers::new());
    let receipt = cqrs
        .execute(&id, &OpenAccount(calls.clone()), EventMetadata::default())
        .await
        .unwrap();
    assert_eq!(receipt.aggregate_id, id);
    assert_eq!(receipt.version, 1);
    assert_eq!(receipt.event_ids, vec!["event-1"]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.appends.load(Ordering::SeqCst), 1);
}
