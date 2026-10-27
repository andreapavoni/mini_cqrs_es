#![cfg(any(feature = "turso", feature = "postgresql"))]
#[path = "support/common_game.rs"]
mod common_game;
#[path = "support/common_hotel.rs"]
mod common_hotel;
#[path = "support/database.rs"]
mod database;
#[path = "support/read_store.rs"]
mod read_store;
use common_hotel::*;
use mini_cqrs_es::{Cqrs, EventMetadata, QueryRunner, SimpleCqrs};
#[cfg(test)]
mod tests {
    use super::*;
    use mini_cqrs_es::{
        AggregateManager, CqrsError, EventConsumer, ExecutionError, LoadedAggregate,
        PostCommitStage, StoredEvent,
    };
    use mini_cqrs_es::{Command, SimpleAggregateManager};
    use mini_cqrs_es::{EventConsumers, EventLog};
    use std::error::Error as StdError;
    use std::fmt;
    use std::str::FromStr;

    async fn setup() -> (
        SimpleCqrs<EventLog, SimpleAggregateManager<EventLog>>,
        mini_cqrs_es::toasty::Db,
        HotelId,
        read_store::Databases,
    ) {
        let fixture = read_store::Databases::temporary().await.unwrap();
        let read_model = fixture.reads.clone();
        let cqrs = SimpleCqrs::from_parts(
            SimpleAggregateManager::new(fixture.events.clone()),
            fixture.events.clone(),
            read_store::projections(&read_model),
        );
        (cqrs, read_model, HotelId::new(1), fixture)
    }

    #[tokio::test]
    async fn test_initialize_hotel() {
        let (cqrs, read_model, hotel_id, _fixture) = setup().await;

        cqrs.execute(
            &hotel_id,
            &CmdInitializeHotel { room_count: 5 },
            EventMetadata::default(),
        )
        .await
        .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(
                hotel_id.clone(),
                read_model.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(state.rooms.len(), 5);
        for i in 1..=5 {
            assert_eq!(state.rooms.get(&i), Some(&RoomState::Free));
        }
    }

    #[tokio::test]
    async fn test_check_in_guest() {
        let (cqrs, read_model, hotel_id, _fixture) = setup().await;

        cqrs.execute(
            &hotel_id,
            &CmdInitializeHotel { room_count: 5 },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
            EventMetadata::default(),
        )
        .await
        .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(
                hotel_id.clone(),
                read_model.clone(),
            ))
            .await
            .unwrap();
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
        let (cqrs, _read_model, hotel_id, _fixture) = setup().await;

        cqrs.execute(
            &hotel_id,
            &CmdInitializeHotel { room_count: 5 },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
            EventMetadata::default(),
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
                EventMetadata::default(),
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::NotCommitted {
                source: CqrsError::Domain(_)
            }
        ));
    }

    #[tokio::test]
    async fn test_check_out_guest() {
        let (cqrs, read_model, hotel_id, _fixture) = setup().await;

        cqrs.execute(
            &hotel_id,
            &CmdInitializeHotel { room_count: 5 },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckOut { room_number: 1 },
            EventMetadata::default(),
        )
        .await
        .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(
                hotel_id.clone(),
                read_model.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(state.rooms.get(&1), Some(&RoomState::Free));
    }

    #[tokio::test]
    async fn test_check_out_free_room_fails() {
        let (cqrs, _read_model, hotel_id, _fixture) = setup().await;

        cqrs.execute(
            &hotel_id,
            &CmdInitializeHotel { room_count: 5 },
            EventMetadata::default(),
        )
        .await
        .unwrap();

        let result = cqrs
            .execute(
                &hotel_id,
                &CmdCheckOut { room_number: 1 },
                EventMetadata::default(),
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::NotCommitted {
                source: CqrsError::Domain(_)
            }
        ));
    }

    #[tokio::test]
    async fn test_multiple_rooms() {
        let (cqrs, read_model, hotel_id, _fixture) = setup().await;

        cqrs.execute(
            &hotel_id,
            &CmdInitializeHotel { room_count: 5 },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 2,
                guest_name: "Bob".into(),
            },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 3,
                guest_name: "Charlie".into(),
            },
            EventMetadata::default(),
        )
        .await
        .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(
                hotel_id.clone(),
                read_model.clone(),
            ))
            .await
            .unwrap();
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
        let (cqrs, read_model, hotel_id, _fixture) = setup().await;

        cqrs.execute(
            &hotel_id,
            &CmdInitializeHotel { room_count: 5 },
            EventMetadata::default(),
        )
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
                EventMetadata::default(),
            )
            .await
            .unwrap();
        }

        // All occupied
        let state = cqrs
            .query(&GetHotelStateQuery::new(
                hotel_id.clone(),
                read_model.clone(),
            ))
            .await
            .unwrap();
        assert!(
            state
                .rooms
                .values()
                .all(|s| matches!(s, RoomState::Occupied { .. }))
        );

        // Check out rooms 2 and 4
        cqrs.execute(
            &hotel_id,
            &CmdCheckOut { room_number: 2 },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckOut { room_number: 4 },
            EventMetadata::default(),
        )
        .await
        .unwrap();

        let state = cqrs
            .query(&GetHotelStateQuery::new(
                hotel_id.clone(),
                read_model.clone(),
            ))
            .await
            .unwrap();
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
        let fixture = read_store::Databases::temporary().await.unwrap();
        let store = fixture.events.clone();

        let read_model = fixture.reads.clone();
        let consumer = mini_cqrs_es::Projector::new(read_model.clone(), HotelProjection);
        let consumers = EventConsumers::new().with(consumer);

        let agg_manager = SimpleAggregateManager::new(store.clone());
        let cqrs = SimpleCqrs::from_parts(agg_manager, store.clone(), consumers);
        let hotel_id = HotelId::new(1);

        // Run operations
        cqrs.execute(
            &hotel_id,
            &CmdInitializeHotel { room_count: 5 },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 1,
                guest_name: "Alice".into(),
            },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckIn {
                room_number: 3,
                guest_name: "Charlie".into(),
            },
            EventMetadata::default(),
        )
        .await
        .unwrap();
        cqrs.execute(
            &hotel_id,
            &CmdCheckOut { room_number: 1 },
            EventMetadata::default(),
        )
        .await
        .unwrap();

        // Get read model state (built by consumer)
        let projected_state = cqrs
            .query(&GetHotelStateQuery::new(
                hotel_id.clone(),
                read_model.clone(),
            ))
            .await
            .unwrap();

        // Replay from event store using a fresh aggregate manager
        let fresh_manager = SimpleAggregateManager::new(store);
        let replayed: LoadedAggregate<HotelAggregate> =
            fresh_manager.load(&hotel_id).await.unwrap();

        // Verify replayed aggregate matches projected read model
        assert_eq!(replayed.state().rooms.len(), projected_state.rooms.len());
        for (room_number, room_state) in &projected_state.rooms {
            assert_eq!(replayed.state().rooms.get(room_number), Some(room_state));
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
    async fn test_consumer_failure_reports_committed_events() {
        let fixture = read_store::Databases::temporary().await.unwrap();
        let store = fixture.events.clone();

        let consumers = EventConsumers::new().with(FailingConsumer);
        let agg_manager = SimpleAggregateManager::new(store.clone());
        let cqrs = SimpleCqrs::from_parts(agg_manager, store, consumers);
        let hotel_id = HotelId::new(7);

        let result = cqrs
            .execute(
                &hotel_id,
                &CmdInitializeHotel { room_count: 3 },
                EventMetadata::default(),
            )
            .await;

        assert!(result.is_err());
        let ExecutionError::Committed {
            receipt,
            stage,
            source,
        } = result.unwrap_err()
        else {
            panic!("expected committed consumer failure");
        };
        assert_eq!(receipt.aggregate_id, hotel_id);
        assert_eq!(receipt.version, 1);
        assert_eq!(receipt.event_ids.len(), 1);
        assert_eq!(stage, PostCommitStage::Consumers);
        assert!(matches!(source, CqrsError::Domain(_)));
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
        let (cqrs, _read_model, hotel_id, _fixture) = setup().await;

        let result = cqrs
            .execute(&hotel_id, &CmdInvariantFailure, EventMetadata::default())
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecutionError::NotCommitted {
                source: CqrsError::CommandInvariant(_)
            }
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
