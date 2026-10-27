#![cfg(any(feature = "turso", feature = "postgresql"))]

#[path = "support/database.rs"]
mod database;

use std::sync::{Arc, Mutex};

use mini_cqrs_es::{
    Aggregate, AggregateManager, Command, Cqrs, CqrsError, EventConsumer, EventConsumers, EventLog,
    EventMetadata, EventPayload, EventStore, ExecutionError, LoadedAggregate,
    SimpleAggregateManager, SimpleCqrs, StoredEvent, replay_stream,
};
use serde::{Deserialize, Serialize};

// Revisions are different Rust types with compatible domain/event formats.
// Namespaces deliberately model unrelated domains that can reuse the same ID.
#[derive(Clone, Debug, Default)]
struct Counter<const NAMESPACE: u8, const REVISION: u8> {
    total: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Increased(u64);

impl std::fmt::Display for Increased {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("increased")
    }
}

impl EventPayload for Increased {
    fn event_type(&self) -> &'static str {
        "increased"
    }
}

impl<const NAMESPACE: u8, const REVISION: u8> Aggregate for Counter<NAMESPACE, REVISION> {
    type Id = u64;
    type Event = Increased;

    fn aggregate_type() -> &'static str {
        match NAMESPACE {
            0 => "example.counter",
            1 => "example.other_counter",
            _ => unreachable!("only two domains are used in this fixture"),
        }
    }

    async fn apply(&mut self, event: &Increased) {
        self.total += event.0;
    }
}

struct Increase<const NAMESPACE: u8, const REVISION: u8>(u64);

impl<const NAMESPACE: u8, const REVISION: u8> Command for Increase<NAMESPACE, REVISION> {
    type Aggregate = Counter<NAMESPACE, REVISION>;

    async fn handle(&self, _: &Self::Aggregate) -> Result<Vec<Increased>, CqrsError> {
        Ok(vec![Increased(self.0)])
    }
}

#[derive(Clone, Default)]
struct Projection(Arc<Mutex<Vec<u64>>>);

impl EventConsumer for Projection {
    async fn process(&self, event: &StoredEvent) -> Result<(), CqrsError> {
        if event.aggregate_type == Counter::<0, 2>::aggregate_type() {
            self.0
                .lock()
                .unwrap()
                .push(event.get_payload::<Increased>()?.0);
        }
        Ok(())
    }
}

async fn database() -> (tempfile::TempDir, String, EventLog) {
    let directory = tempfile::tempdir().unwrap();
    let url = database::fresh_url(&directory.path().join("events.db"), false).await;
    let store = EventLog::connect(&url).await.unwrap();
    store.database().push_schema().await.unwrap();
    (directory, url, store)
}

#[tokio::test]
async fn compatible_rust_types_continue_and_replay_the_same_persisted_stream() {
    type Previous = Counter<0, 1>;
    type Renamed = Counter<0, 2>;
    assert_ne!(
        std::any::type_name::<Previous>(),
        std::any::type_name::<Renamed>()
    );
    assert_eq!(Previous::aggregate_type(), Renamed::aggregate_type());

    let (_directory, url, store) = database().await;
    let before = {
        let engine = SimpleCqrs::from_parts(
            SimpleAggregateManager::new(store.clone()),
            store.clone(),
            EventConsumers::new(),
        );
        let receipt = engine
            .execute(&42, &Increase::<0, 1>(4), EventMetadata::default())
            .await
            .unwrap();
        assert_eq!(receipt.version, 1);
        let history = store
            .load_events(Previous::aggregate_type(), "42")
            .await
            .unwrap()
            .0;
        assert_eq!(receipt.event_ids, vec![history[0].id.clone()]);
        serde_json::to_value(history).unwrap()
    };
    drop(store);

    let reopened = EventLog::connect(&url).await.unwrap();
    let manager = SimpleAggregateManager::new(reopened.clone());
    let renamed: LoadedAggregate<Renamed> = manager.load(&42).await.unwrap();
    assert_eq!(
        (*renamed.id(), renamed.version(), renamed.state().total),
        (42, 1, 4)
    );
    let engine = SimpleCqrs::from_parts(manager, reopened.clone(), EventConsumers::new());
    let receipt = engine
        .execute(&42, &Increase::<0, 2>(5), EventMetadata::default())
        .await
        .unwrap();
    assert_eq!(receipt.aggregate_id, 42);
    assert_eq!(receipt.version, 2);
    let (history, tip) = reopened
        .load_events(Renamed::aggregate_type(), "42")
        .await
        .unwrap();
    assert_eq!(tip, 2);
    assert_eq!(serde_json::to_value(&history[..1]).unwrap(), before);
    assert_eq!(receipt.event_ids, vec![history[1].id.clone()]);
    assert_eq!(
        reopened.list_streams().await.unwrap(),
        vec![("example.counter".into(), "42".into())]
    );
    // Neither Rust type name was accidentally used as another stream namespace.
    for kind in [
        std::any::type_name::<Previous>(),
        std::any::type_name::<Renamed>(),
    ] {
        assert!(reopened.load_events(kind, "42").await.unwrap().0.is_empty());
    }
    let manager = SimpleAggregateManager::new(reopened.clone());
    let original: LoadedAggregate<Previous> = manager.load(&42).await.unwrap();
    assert_eq!((original.version(), original.state().total), (2, 9));

    let projection = Projection::default();
    let consumers = EventConsumers::new().with(projection.clone());
    assert_eq!(
        replay_stream(&reopened, Renamed::aggregate_type(), "42", &consumers)
            .await
            .unwrap(),
        2
    );
    assert_eq!(*projection.0.lock().unwrap(), vec![4, 5]);
    assert_eq!(
        serde_json::to_value(
            reopened
                .load_events(Renamed::aggregate_type(), "42")
                .await
                .unwrap()
                .0
        )
        .unwrap(),
        serde_json::to_value(history).unwrap(),
    );
}

#[tokio::test]
async fn different_names_are_isolated_and_do_not_rename_existing_history() {
    let (_directory, _url, store) = database().await;
    let manager = SimpleAggregateManager::new(store.clone());
    let engine = SimpleCqrs::from_parts(
        SimpleAggregateManager::new(store.clone()),
        store.clone(),
        EventConsumers::new(),
    );
    engine
        .execute(&42, &Increase::<0, 1>(4), EventMetadata::default())
        .await
        .unwrap();
    let before = store
        .load_events(Counter::<0, 1>::aggregate_type(), "42")
        .await
        .unwrap()
        .0;
    let unrelated: LoadedAggregate<Counter<1, 1>> = manager.load(&42).await.unwrap();
    assert_eq!((unrelated.version(), unrelated.state().total), (0, 0));
    let receipt = engine
        .execute(&42, &Increase::<1, 1>(99), EventMetadata::default())
        .await
        .unwrap();
    assert_eq!(receipt.version, 1);
    let unrelated: LoadedAggregate<Counter<1, 2>> = manager.load(&42).await.unwrap();
    assert_eq!((unrelated.version(), unrelated.state().total), (1, 99));
    assert_eq!(
        serde_json::to_value(
            store
                .load_events(Counter::<0, 1>::aggregate_type(), "42")
                .await
                .unwrap()
                .0
        )
        .unwrap(),
        serde_json::to_value(before).unwrap(),
    );
    assert_eq!(store.list_streams().await.unwrap().len(), 2);
}

struct StaleManager;

impl AggregateManager<Counter<0, 1>> for StaleManager {
    async fn load(&self, id: &u64) -> Result<LoadedAggregate<Counter<0, 1>>, CqrsError> {
        Ok(LoadedAggregate::new(*id, 0, Counter::default()))
    }
}

#[tokio::test]
async fn stale_runtime_version_is_rejected_without_overwriting_event_history() {
    let (_directory, _url, store) = database().await;
    let engine = SimpleCqrs::from_parts(
        SimpleAggregateManager::new(store.clone()),
        store.clone(),
        EventConsumers::new(),
    );
    engine
        .execute(&42, &Increase::<0, 1>(4), EventMetadata::default())
        .await
        .unwrap();
    let history = store
        .load_events(Counter::<0, 1>::aggregate_type(), "42")
        .await
        .unwrap()
        .0;
    let stale = SimpleCqrs::from_parts(StaleManager, store.clone(), EventConsumers::new());
    let error = stale
        .execute(&42, &Increase::<0, 1>(99), EventMetadata::default())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ExecutionError::NotCommitted {
            source: CqrsError::Conflict {
                expected_version: 0,
                actual_version: 1
            }
        }
    ));
    assert_eq!(
        serde_json::to_value(
            store
                .load_events(Counter::<0, 1>::aggregate_type(), "42")
                .await
                .unwrap()
                .0
        )
        .unwrap(),
        serde_json::to_value(history).unwrap(),
    );
}
