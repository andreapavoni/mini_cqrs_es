/// This is the same `game` example, but it uses aggregate snapshots instead of events to rebuild
/// the aggregate.
use std::sync::Arc;

use tokio::sync::Mutex;

use mini_cqrs::{ Cqrs, SnapshotDispatcher, QueriesRunner };

#[path = "common_game.rs"]
mod common_game;
use common_game::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();
    let repo = Arc::new(Mutex::new(InMemoryRepository::new()));
    let snapshot_store = InMemorySnapshotStore::new();

    let consumers = vec![GameEventConsumers::Counter(CounterConsumer::new(repo.clone()))];
    let dispatcher: GameSnapshotDispatcher = SnapshotDispatcher::new(store, snapshot_store, consumers);
    let queries = AppQueries {};
    let mut cqrs = Cqrs::new(dispatcher, queries);

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

    let q = GetGameQuery::new(main_id.clone(), repo.clone());

    let result = cqrs.queries().run(q.clone()).await?.unwrap();

    assert_eq!(result.player_1.id, "player_1".to_string());
    assert_eq!(result.player_2.id, "player_2".to_string());
    verify_game_result(&result, 0, 0, 3, GameStatus::Playing);

    let command = GameCommand::AttackPlayer {
        attacker: player_1.clone(),
    };

    cqrs.execute(main_id.clone(), command.clone()).await?;

    let result = cqrs.queries().run(q.clone()).await?.unwrap();
    verify_game_result(&result, 1, 0, 3, GameStatus::Playing);

    cqrs.execute(main_id.clone(), command.clone()).await?;

    let result = cqrs.queries().run(q.clone()).await?.unwrap();
    verify_game_result(&result, 2, 0, 3, GameStatus::Playing);

    cqrs.execute(
        main_id.clone(),
        GameCommand::AttackPlayer {
            attacker: player_2.clone(),
        },
    )
    .await?;

    let result = cqrs.queries().run(q.clone()).await?.unwrap();
    verify_game_result(&result, 2, 1, 3, GameStatus::Playing);

    cqrs.execute(main_id.clone(), command.clone()).await?;

    let winner = Player {
        id: player_1.id.clone(),
        points: 3,
    };

    let result = cqrs.queries().run(q.clone()).await?.unwrap();
    verify_game_result(&result, 3, 1, 3, GameStatus::Winner(winner));
    Ok(())
}
