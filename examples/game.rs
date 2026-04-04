/// # MiniCQRS/ES Example: Game
///
/// ## Usage
///
/// ```sh
/// cargo run --example game
/// ```
///
use std::sync::{Arc, Mutex};

use mini_cqrs_es::{Cqrs, EventConsumers, QueryRunner, SimpleCqrs, SnapshotAggregateManager, Uuid};

#[path = "lib/common_game.rs"]
mod common_game;
use common_game::*;

#[tokio::main(flavor = "current_thread")]
async fn main() -> mini_cqrs_es::anyhow::Result<()> {
    let store = InMemoryEventStore::new();
    let repo = Arc::new(Mutex::new(InMemoryRepository::new()));
    let snapshot_store = InMemorySnapshotStore::<GameAggregate>::new();

    let consumers = EventConsumers::new()
        .with(GameMainConsumer::new(repo.clone()))
        .with(PrintEventConsumer {});

    let aggregate_manager = SnapshotAggregateManager::new(snapshot_store);

    let cqrs = SimpleCqrs::new(aggregate_manager, store, consumers);

    let player_1 = Player {
        id: "player_1".to_string(),
        points: 0,
    };
    let player_2 = Player {
        id: "player_2".to_string(),
        points: 0,
    };

    let aggregate_id = Uuid::new_v4();

    let start_cmd = CmdStartGame {
        player_1: player_1.clone(),
        player_2: player_2.clone(),
        goal: 3,
    };

    let result_id = cqrs.execute(aggregate_id, &start_cmd).await?;
    assert_eq!(result_id, aggregate_id);
    let q = GetGameQuery::new(aggregate_id, repo.clone());
    let result = cqrs.query(&q).await?.unwrap();

    assert_eq!(result.player_1.id, player_1.id);
    assert_eq!(result.player_2.id, player_2.id);
    verify_game_result(&result, 0, 0, 3, GameStatus::Playing);

    let attack_cmd = CmdAttackPlayer {
        attacker: player_1.clone(),
    };
    cqrs.execute(aggregate_id, &attack_cmd).await?;
    let result = cqrs.query(&q).await?.unwrap();

    verify_game_result(&result, 1, 0, 3, GameStatus::Playing);

    cqrs.execute(aggregate_id, &attack_cmd).await?;
    let result = cqrs.query(&q).await?.unwrap();

    verify_game_result(&result, 2, 0, 3, GameStatus::Playing);

    let attack_cmd_2 = CmdAttackPlayer {
        attacker: player_2.clone(),
    };
    cqrs.execute(aggregate_id, &attack_cmd_2).await?;
    let result = cqrs.query(&q).await?.unwrap();

    verify_game_result(&result, 2, 1, 3, GameStatus::Playing);

    cqrs.execute(aggregate_id, &attack_cmd).await?;
    let winner = Player {
        id: player_1.id.clone(),
        points: 3,
    };
    let result = cqrs.query(&q).await?.unwrap();

    verify_game_result(&result, 3, 1, 3, GameStatus::Winner(winner));
    Ok(())
}
