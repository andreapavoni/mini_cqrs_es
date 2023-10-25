/// # MiniCQRS/ES Example: Game
///
/// ## Usage
///
/// ```sh
/// cargo run --example game
/// ```
///
use std::sync::Arc;

use mini_cqrs_es::{Cqrs, QueriesRunner, SnapshotAggregateManager, Uuid};
use tokio::sync::Mutex;

#[path = "lib/common_game.rs"]
mod common_game;
use common_game::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();
    let repo = Arc::new(Mutex::new(InMemoryRepository::new()));
    let snapshot_store = InMemorySnapshotStore::<GameState>::new();

    let consumers = GameEventConsumersGroup {
        main: GameMainConsumer::new(repo.clone()),
        print: PrintEventConsumer {},
    };

    let aggregate_manager = SnapshotAggregateManager::new(snapshot_store);

    let mut cqrs = Cqrs::new(aggregate_manager, store, consumers);

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

    cqrs.execute(main_id, &start_cmd).await?;
    let q = GetGameQuery::new(main_id, repo.clone());
    let result = cqrs.query(&q).await?.unwrap();

    assert_eq!(result.player_1.id, player_1.id);
    assert_eq!(result.player_2.id, player_2.id);
    verify_game_result(&result, 0, 0, 3, GameStatus::Playing);

    let attack_cmd = CmdAttackPlayer {
        attacker: player_1.clone(),
    };
    cqrs.execute(main_id, &attack_cmd).await?;
    let result = cqrs.query(&q).await?.unwrap();

    verify_game_result(&result, 1, 0, 3, GameStatus::Playing);

    cqrs.execute(main_id, &attack_cmd).await?;
    let result = cqrs.query(&q).await?.unwrap();

    verify_game_result(&result, 2, 0, 3, GameStatus::Playing);

    let attack_cmd_2 = CmdAttackPlayer {
        attacker: player_2.clone(),
    };
    cqrs.execute(main_id, &attack_cmd_2).await?;
    let result = cqrs.query(&q).await?.unwrap();

    verify_game_result(&result, 2, 1, 3, GameStatus::Playing);

    cqrs.execute(main_id, &attack_cmd).await?;
    let winner = Player {
        id: player_1.id.clone(),
        points: 3,
    };
    let result = cqrs.query(&q).await?.unwrap();

    verify_game_result(&result, 3, 1, 3, GameStatus::Winner(winner));
    Ok(())
}
