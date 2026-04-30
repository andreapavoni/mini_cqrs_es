/// # MiniCQRS/ES Example: Hotel
///
/// A simplified hotel with 5 rooms, using SQLite (sqlx) for event storage.
///
/// ## Usage
///
/// ```sh
/// cargo run --example hotel
/// cargo test --example hotel
/// ```
///
use std::sync::{Arc, Mutex};

use mini_cqrs_es::{Cqrs, EventConsumers, QueryRunner, SimpleCqrs, SimpleAggregateManager};
use sqlx::SqlitePool;

#[path = "lib/common_hotel.rs"]
mod common_hotel;
use common_hotel::*;

#[tokio::main(flavor = "current_thread")]
async fn main() -> mini_cqrs_es::anyhow::Result<()> {
    let pool = SqlitePool::connect("sqlite::memory:").await?;
    let store = SqliteEventStore::new(pool);
    store.create_table().await?;

    let read_model = Arc::new(Mutex::new(HotelReadModel::default()));
    let consumer = HotelProjectionConsumer::new(read_model.clone());
    let consumers = EventConsumers::new().with(consumer);

    let agg_manager = SimpleAggregateManager::new(store.clone());
    let cqrs = SimpleCqrs::new(agg_manager, store, consumers);
    let hotel_id = HotelId::new(1);

    // Initialize hotel with 5 rooms
    cqrs.execute(&hotel_id, &CmdInitializeHotel { room_count: 5 })
        .await?;
    println!("Hotel initialized with 5 rooms.");

    // Check in some guests
    cqrs.execute(
        &hotel_id,
        &CmdCheckIn {
            room_number: 1,
            guest_name: "Alice".into(),
        },
    )
    .await?;
    cqrs.execute(
        &hotel_id,
        &CmdCheckIn {
            room_number: 3,
            guest_name: "Bob".into(),
        },
    )
    .await?;

    let state = cqrs
        .query(&GetHotelStateQuery::new(read_model.clone()))
        .await;
    println!("After check-ins: {:?}", state.rooms);

    // Check out Alice
    cqrs.execute(&hotel_id, &CmdCheckOut { room_number: 1 })
        .await?;

    let state = cqrs
        .query(&GetHotelStateQuery::new(read_model.clone()))
        .await;
    println!("After Alice checks out: {:?}", state.rooms);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mini_cqrs_es::Command;
    use mini_cqrs_es::{AggregateManager, CqrsError, EventConsumer, StoredEvent};
    use std::error::Error as StdError;
    use std::fmt;
    use std::str::FromStr;

    async fn setup() -> (
        SimpleCqrs<SqliteEventStore, SimpleAggregateManager<SqliteEventStore>>,
        Arc<Mutex<HotelReadModel>>,
        HotelId,
    ) {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let store = SqliteEventStore::new(pool);
        store.create_table().await.unwrap();

        let read_model = Arc::new(Mutex::new(HotelReadModel::default()));
        let consumer = HotelProjectionConsumer::new(read_model.clone());
        let consumers = EventConsumers::new().with(consumer);

        let agg_manager = SimpleAggregateManager::new(store.clone());
        let cqrs = SimpleCqrs::new(agg_manager, store, consumers);
        let hotel_id = HotelId::new(1);

        (cqrs, read_model, hotel_id)
    }

    #[tokio::test]
    async fn test_initialize_hotel() {
        let (cqrs, read_model, hotel_id) = setup().await;

        cqrs.execute(&hotel_id, &CmdInitializeHotel { room_count: 5 })
            .await
            .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(read_model.clone()))
            .await;
        assert_eq!(state.rooms.len(), 5);
        for i in 1..=5 {
            assert_eq!(state.rooms.get(&i), Some(&RoomState::Free));
        }
    }

    #[tokio::test]
    async fn test_check_in_guest() {
        let (cqrs, read_model, hotel_id) = setup().await;

        cqrs.execute(&hotel_id, &CmdInitializeHotel { room_count: 5 })
            .await
            .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
        )
        .await
        .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(read_model.clone()))
            .await;
        assert_eq!(
            state.rooms.get(&1),
            Some(&RoomState::Occupied {
                guest_name: "Alice".into()
            })
        );
        for i in 2..=5 {
            assert_eq!(state.rooms.get(&i), Some(&RoomState::Free));
        }
    }

    #[tokio::test]
    async fn test_check_in_occupied_room_fails() {
        let (cqrs, _read_model, hotel_id) = setup().await;

        cqrs.execute(&hotel_id, &CmdInitializeHotel { room_count: 5 })
            .await
            .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
        )
        .await
        .unwrap();

        let result = cqrs
            .execute(
                &hotel_id,
                &CmdCheckIn {
                    room_number: 1,
                    guest_name: "Bob".into(),
                },
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CqrsError::Domain(_)));
    }

    #[tokio::test]
    async fn test_check_out_guest() {
        let (cqrs, read_model, hotel_id) = setup().await;

        cqrs.execute(&hotel_id, &CmdInitializeHotel { room_count: 5 })
            .await
            .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
        )
        .await
        .unwrap();
        cqrs.execute(&hotel_id, &CmdCheckOut { room_number: 1 })
            .await
            .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(read_model.clone()))
            .await;
        assert_eq!(state.rooms.get(&1), Some(&RoomState::Free));
    }

    #[tokio::test]
    async fn test_check_out_free_room_fails() {
        let (cqrs, _read_model, hotel_id) = setup().await;

        cqrs.execute(&hotel_id, &CmdInitializeHotel { room_count: 5 })
            .await
            .unwrap();

        let result = cqrs
            .execute(&hotel_id, &CmdCheckOut { room_number: 1 })
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CqrsError::Domain(_)));
    }

    #[tokio::test]
    async fn test_multiple_rooms() {
        let (cqrs, read_model, hotel_id) = setup().await;

        cqrs.execute(&hotel_id, &CmdInitializeHotel { room_count: 5 })
            .await
            .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 2,
                guest_name: "Bob".into(),
            },
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 3,
                guest_name: "Charlie".into(),
            },
        )
        .await
        .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(read_model.clone()))
            .await;
        let occupied_count = state
            .rooms
            .values()
            .filter(|s| matches!(s, RoomState::Occupied { .. }))
            .count();
        let free_count = state
            .rooms
            .values()
            .filter(|s| matches!(s, RoomState::Free))
            .count();
        assert_eq!(occupied_count, 3);
        assert_eq!(free_count, 2);
    }

    #[tokio::test]
    async fn test_full_lifecycle() {
        let (cqrs, read_model, hotel_id) = setup().await;

        cqrs.execute(&hotel_id, &CmdInitializeHotel { room_count: 5 })
            .await
            .unwrap();

        // Check in all 5 rooms
        for (i, name) in [
            (1, "Alice"),
            (2, "Bob"),
            (3, "Charlie"),
            (4, "Diana"),
            (5, "Eve"),
        ] {
            cqrs.execute(
                &hotel_id,
                &CmdCheckIn {
                    room_number: i,
                    guest_name: name.into(),
                },
            )
            .await
            .unwrap();
        }

        // All occupied
        let state = cqrs
            .query(&GetHotelStateQuery::new(read_model.clone()))
            .await;
        assert!(state
            .rooms
            .values()
            .all(|s| matches!(s, RoomState::Occupied { .. })));

        // Check out rooms 2 and 4
        cqrs.execute(&hotel_id, &CmdCheckOut { room_number: 2 })
            .await
            .unwrap();
        cqrs.execute(&hotel_id, &CmdCheckOut { room_number: 4 })
            .await
            .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(read_model.clone()))
            .await;
        let occupied_count = state
            .rooms
            .values()
            .filter(|s| matches!(s, RoomState::Occupied { .. }))
            .count();
        let free_count = state
            .rooms
            .values()
            .filter(|s| matches!(s, RoomState::Free))
            .count();
        assert_eq!(occupied_count, 3);
        assert_eq!(free_count, 2);
        assert_eq!(state.rooms.get(&2), Some(&RoomState::Free));
        assert_eq!(state.rooms.get(&4), Some(&RoomState::Free));
        assert_eq!(
            state.rooms.get(&1),
            Some(&RoomState::Occupied {
                guest_name: "Alice".into()
            })
        );
    }

    #[tokio::test]
    async fn test_event_replay_consistency() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let store = SqliteEventStore::new(pool);
        store.create_table().await.unwrap();

        let read_model = Arc::new(Mutex::new(HotelReadModel::default()));
        let consumer = HotelProjectionConsumer::new(read_model.clone());
        let consumers = EventConsumers::new().with(consumer);

        let agg_manager = SimpleAggregateManager::new(store.clone());
        let cqrs = SimpleCqrs::new(agg_manager, store.clone(), consumers);
        let hotel_id = HotelId::new(1);

        // Run operations
        cqrs.execute(&hotel_id, &CmdInitializeHotel { room_count: 5 })
            .await
            .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 3,
                guest_name: "Charlie".into(),
            },
        )
        .await
        .unwrap();
        cqrs.execute(&hotel_id, &CmdCheckOut { room_number: 1 })
            .await
            .unwrap();

        // Get read model state (built by consumer)
        let projected_state = cqrs
            .query(&GetHotelStateQuery::new(read_model.clone()))
            .await;

        // Replay from event store using a fresh aggregate manager
        let fresh_manager = SimpleAggregateManager::new(store);
        let replayed: HotelAggregate = fresh_manager.load(&hotel_id).await.unwrap();

        // Verify replayed aggregate matches projected read model
        assert_eq!(replayed.rooms.len(), projected_state.rooms.len());
        for (room_number, room_state) in &projected_state.rooms {
            assert_eq!(replayed.rooms.get(room_number), Some(room_state));
        }
    }

    #[test]
    fn test_hotel_id_display_fromstr_roundtrip() {
        let id = HotelId::new(42);
        let rendered = id.to_string();
        let parsed = HotelId::from_str(&rendered).unwrap();
        assert_eq!(parsed, id);
    }

    #[derive(Clone)]
    struct FailingConsumer;

    impl EventConsumer for FailingConsumer {
        async fn process(&self, _event: &StoredEvent) -> Result<(), CqrsError> {
            Err(CqrsError::domain("projection failure"))
        }
    }

    #[tokio::test]
    async fn test_consumer_failure_aborts_execute() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let store = SqliteEventStore::new(pool);
        store.create_table().await.unwrap();

        let consumers = EventConsumers::new().with(FailingConsumer);
        let agg_manager = SimpleAggregateManager::new(store.clone());
        let cqrs = SimpleCqrs::new(agg_manager, store, consumers);
        let hotel_id = HotelId::new(7);

        let result = cqrs
            .execute(&hotel_id, &CmdInitializeHotel { room_count: 3 })
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CqrsError::Domain(_)));
    }

    struct CmdInvariantFailure;

    impl Command for CmdInvariantFailure {
        type Aggregate = HotelAggregate;

        async fn handle(&self, _aggregate: &Self::Aggregate) -> Result<Vec<HotelEvent>, CqrsError> {
            Err(CqrsError::invariant("stale command target"))
        }
    }

    #[tokio::test]
    async fn test_command_invariant_failure_is_distinct_from_domain_failure() {
        let (cqrs, _read_model, hotel_id) = setup().await;

        let result = cqrs.execute(&hotel_id, &CmdInvariantFailure).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CqrsError::CommandInvariant(_)
        ));
    }

    #[derive(Debug)]
    struct AppInvariantError;

    impl fmt::Display for AppInvariantError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "aggregate target mismatch")
        }
    }

    impl StdError for AppInvariantError {}

    #[test]
    fn test_invariant_source_preserves_display_and_source() {
        let error = CqrsError::invariant_source(AppInvariantError);

        assert_eq!(error.to_string(), "aggregate target mismatch");
        assert!(StdError::source(&error).is_some());
    }
}
