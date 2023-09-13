/// This is the same `game` example, but it uses aggregate snapshots instead of events to rebuild
/// the aggregate.
use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;

use mini_cqrs::{
    Aggregate, AggregateSnapshot, Cqrs, CqrsError, SnapshotDispatcher, SnapshotStore,
};

#[path = "common.rs"]
mod common;
use common::*;

#[path = "common_game.rs"]
mod common_game;
use common_game::*;

// Event Store
struct InMemorySnapshotStore<T: Aggregate> {
    snapshots: HashMap<String, AggregateSnapshot<T>>,
}

impl<T: Aggregate> InMemorySnapshotStore<T> {
    pub fn new() -> Self {
        InMemorySnapshotStore {
            snapshots: HashMap::new(),
        }
    }
}

#[async_trait]
impl SnapshotStore<GameState> for InMemorySnapshotStore<GameState> {
    async fn save_snapshot(
        &mut self,
        snapshot: AggregateSnapshot<GameState>,
    ) -> Result<(), CqrsError> {
        self.snapshots
            .insert(snapshot.clone().aggregate_id, snapshot);
        Ok(())
    }

    async fn load_snapshot(
        &self,
        aggregate_id: <GameState as Aggregate>::Id,
    ) -> Result<AggregateSnapshot<GameState>, CqrsError> {
        if let Some(snapshot) = self.snapshots.get(&aggregate_id) {
            Ok(snapshot.clone())
        } else {
            Err(CqrsError::new(format!(
                "No events for aggregate id `{}`",
                aggregate_id
            )))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();
    let repo = Arc::new(Mutex::new(InMemoryRepository::new()));
    let snapshot_store = InMemorySnapshotStore::new();

    let consumers = vec![GameEventConsumers::Counter(CounterConsumer::new(repo.clone()))];

    let dispatcher: SnapshotDispatcher<
        GameState,
        InMemoryEventStore,
        InMemorySnapshotStore<GameState>,
        GameEventConsumers,
    > = SnapshotDispatcher::new(store, snapshot_store, consumers);

    let read_model = GameView::new(repo);

    let mut cqrs = Cqrs::new(dispatcher);

    let player_1 = Player {
        id: "player_1".to_string(),
        points: 0,
    };
    let player_2 = Player {
        id: "player_2".to_string(),
        points: 0,
    };

    let main_id = "1".to_string();

    cqrs.execute(
        main_id.clone(),
        GameCommand::StartGame {
            player_1: player_1.clone(),
            player_2: player_2.clone(),
            goal: 3,
        },
    )
    .await?;

    let query = GameQuery::GetGame(main_id.clone());

    let result = cqrs
        .query::<GameView>(read_model.clone(), query.clone())
        .await?;
    assert_eq!(result.player_1.id, "player_1".to_string());
    assert_eq!(result.player_2.id, "player_2".to_string());
    verify_game_result(&result, 0, 0, 3, GameStatus::Playing);

    let command = GameCommand::AttackPlayer {
        attacker: player_1.clone(),
    };

    cqrs.execute(main_id.clone(), command.clone()).await?;
    let result = cqrs
        .query::<GameView>(read_model.clone(), query.clone())
        .await?;
    verify_game_result(&result, 1, 0, 3, GameStatus::Playing);

    cqrs.execute(main_id.clone(), command.clone()).await?;
    let result = cqrs
        .query::<GameView>(read_model.clone(), query.clone())
        .await?;
    verify_game_result(&result, 2, 0, 3, GameStatus::Playing);

    cqrs.execute(
        main_id.clone(),
        GameCommand::AttackPlayer {
            attacker: player_2.clone(),
        },
    )
    .await?;
    let result = cqrs
        .query::<GameView>(read_model.clone(), query.clone())
        .await?;
    verify_game_result(&result, 2, 1, 3, GameStatus::Playing);

    cqrs.execute(main_id.clone(), command.clone()).await?;

    let winner = Player {
        id: player_1.id.clone(),
        points: 3,
    };

    let result = cqrs
        .query::<GameView>(read_model.clone(), query.clone())
        .await?;
    verify_game_result(&result, 3, 1, 3, GameStatus::Winner(winner));

    Ok(())
}

fn verify_game_result(
    game: &GameModel,
    player_1_points: u32,
    player_2_points: u32,
    goal: u32,
    status: GameStatus,
) {
    assert_eq!(game.player_1.points, player_1_points);
    assert_eq!(game.player_2.points, player_2_points);
    assert_eq!(game.goal, goal);
    assert_eq!(game.status, status);
}
