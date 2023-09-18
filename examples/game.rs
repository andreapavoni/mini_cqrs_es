/// # Mini CQRS Example: Simple Game
///
/// This example demonstrates a more complex use case by introducing a read model with a repository
/// that can read and write to an in-memory database through an event consumer. While the
/// implementation is basic, it serves as a foundation for exploring and testing more realistic use
/// cases.
///
/// ## Overview
///
/// In this example, we simulate a simple game with the following components:
///
/// - **Game**: Represents the game state, including two players, a score goal, and game actions.
/// - **Players**: Two players compete to reach the score goal.
/// - **Actions**: Players can attack each other, earning points with each successful attack.
/// - **Win Condition**: The first player to reach the score goal wins the game.
///
///
/// ## Usage
///
/// ```sh
/// cargo run --example game
/// ```
/// You won't see nothing in the output, but that's because the code uses `assert`s to check that
/// it behaves as expected. The only output, if any, would be errors.
///

use std::sync::Arc;

use tokio::sync::Mutex;

use mini_cqrs::{Cqrs, SimpleDispatcher, QueriesRunner};

#[path = "lib/common_game.rs"]
mod common_game;
use common_game::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();
    let repo = Arc::new(Mutex::new(InMemoryRepository::new()));

    let consumers = vec![GameEventConsumers::Counter(CounterConsumer::new(repo.clone()))];
    let dispatcher: GameDispatcher = SimpleDispatcher::new(store, consumers);
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

