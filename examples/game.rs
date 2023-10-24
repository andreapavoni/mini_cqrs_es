/// # Mini CQRS Example: Game with SnapshotAggregateManager
///
/// ## Usage
///
/// ```sh
/// cargo run --example game
/// ```
///
use std::sync::Arc;

use tokio::sync::Mutex;

use mini_cqrs::{Cqrs, SnapshotAggregateManager};

use mini_cqrs::QueriesRunner;

#[path = "lib/common_game.rs"]
mod common_game;
use common_game::*;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();
    let repo = Arc::new(Mutex::new(InMemoryRepository::new()));
    let snapshot_store = InMemorySnapshotStore::<GameState>::new();

    let consumers = vec![GameEventConsumers::Counter(CounterConsumer::new(
        repo.clone(),
    ))];

    let aggregate_manager = SnapshotAggregateManager::new(snapshot_store);

    let mut cqrs = Cqrs::new(aggregate_manager, store.clone(), consumers);

    let player_1 = Player {
        id: "player_1".to_string(),
        points: 0,
    };
    let player_2 = Player {
        id: "player_2".to_string(),
        points: 0,
    };

    let main_id = Uuid::new_v4();

    let start_cmd = CmdStartGame {
        player_1: player_1.clone(),
        player_2: player_2.clone(),
        goal: 3,
    };

    cqrs.execute(main_id.clone(), start_cmd).await?;
    let q = GetGameQuery::new(main_id.clone(), repo.clone());
    let result = cqrs.query(q.clone()).await?.unwrap();

    assert_eq!(result.player_1.id, "player_1".to_string());
    assert_eq!(result.player_2.id, "player_2".to_string());
    verify_game_result(&result, 0, 0, 3, GameStatus::Playing);

    let attack_cmd = CmdAttackPlayer { attacker: player_1.clone(), };
    cqrs.execute(main_id.clone(), attack_cmd.clone()).await?;
    let result = cqrs.query(q.clone()).await?.unwrap();

    verify_game_result(&result, 1, 0, 3, GameStatus::Playing);

    cqrs.execute(main_id.clone(), attack_cmd.clone()).await?;
    let result = cqrs.query(q.clone()).await?.unwrap();

    verify_game_result(&result, 2, 0, 3, GameStatus::Playing);

    let attack_cmd_2 = CmdAttackPlayer {
        attacker: player_2.clone(),
    };
    cqrs.execute(main_id.clone(), attack_cmd_2).await?;
    let result = cqrs.query(q.clone()).await?.unwrap();

    verify_game_result(&result, 2, 1, 3, GameStatus::Playing);

    cqrs.execute(main_id.clone(), attack_cmd.clone()).await?;
    let winner = Player {
        id: player_1.id.clone(),
        points: 3,
    };
    let result = cqrs.query(q.clone()).await?.unwrap();

    verify_game_result(&result, 3, 1, 3, GameStatus::Winner(winner));
    Ok(())
}
