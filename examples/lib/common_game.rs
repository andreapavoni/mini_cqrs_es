#![allow(dead_code)]

// Common code shared in the examples to avoid repetitions and focus on the core concepts

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use mini_cqrs::{
    event_consumers_group, wrap_event, Aggregate, AggregateSnapshot, CqrsError, Event,
    EventConsumer, EventConsumersGroup, EventPayload, ModelReader, QueriesRunner, Query,
    Repository, SimpleDispatcher, SnapshotDispatcher, SnapshotStore,
};

#[path = "common.rs"]
mod common;
pub use common::*;

pub struct InMemorySnapshotStore<T: Aggregate> {
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

// Commands: for demonstration purposes, we can only start the game or attack the opponent.
#[derive(PartialEq, Clone)]
pub enum GameCommand {
    StartGame {
        player_1: Player,
        player_2: Player,
        goal: u32,
    },
    AttackPlayer {
        attacker: Player,
    },
}

// Events: the outcomes of the above commands, including the end of the game with a winner.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    GameStarted {
        aggregate_id: String,
        player_1: Player,
        player_2: Player,
        goal: u32,
    },
    PlayerAttacked {
        aggregate_id: String,
        attacker: Player,
    },
    GameEndedWithWinner {
        aggregate_id: String,
        winner: Player,
    },
}

wrap_event!(GameEvent);

impl ToString for GameEvent {
    fn to_string(&self) -> String {
        match self {
            GameEvent::GameStarted { .. } => "GameStarted".to_string(),
            GameEvent::PlayerAttacked { .. } => "PlayerAttacked".to_string(),
            GameEvent::GameEndedWithWinner { .. } => "GameEndedWithWinner".to_string(),
        }
    }
}

impl EventPayload for GameEvent {
    fn aggregate_id(&self) -> String {
        match self {
            GameEvent::GameStarted { aggregate_id, .. } => aggregate_id.clone(),
            GameEvent::PlayerAttacked { aggregate_id, .. } => aggregate_id.clone(),
            GameEvent::GameEndedWithWinner { aggregate_id, .. } => aggregate_id.clone(),
        }
    }
}

// Aggregate: it's a more complex data structure with structs and enums as field values.
#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub id: String,
    pub points: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GameStatus {
    Playing,
    Winner(Player),
}

// Game aggregate
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameState {
    id: String,
    player_1: Player,
    player_2: Player,
    status: GameStatus,
    goal: u32,
}

// We don't care about the default starting values, any placeholder will be ok, because the real
// data will be loaded/stored when the game is started.
impl Default for GameState {
    fn default() -> Self {
        Self {
            id: String::new(),
            player_1: Player {
                id: "player_1".to_string(),
                points: 0,
            },
            player_2: Player {
                id: "player_2".to_string(),
                points: 0,
            },
            status: GameStatus::Playing,
            goal: 0,
        }
    }
}

#[async_trait]
impl Aggregate for GameState {
    type Command = GameCommand;
    type Event = GameEvent;
    type Id = String;

    async fn handle(&self, command: Self::Command) -> Result<Vec<Event>, CqrsError> {
        // Here's the realm for the business logic of any sorts, it can either be executed before
        // checking the command type, or inside the command handler itself.

        if self.status != GameStatus::Playing {
            return Err(CqrsError::new(format!(
                "Game is already finished with state {:?}",
                self.status
            )));
        }

        match command {
            // This command will emit an event with the correct data to populate the aggregate.
            GameCommand::StartGame {
                player_1,
                player_2,
                goal,
            } => Ok(vec![GameEvent::GameStarted {
                aggregate_id: self.id.clone(),
                player_1,
                player_2,
                goal,
            }
            .into()]),
            GameCommand::AttackPlayer { attacker } => {
                let mut player = if self.player_1.id == attacker.id {
                    self.player_1.clone()
                } else {
                    self.player_2.clone()
                };

                player.points += 1;

                // First event: a player attacks its opponent and increases its points.
                let mut events: Vec<Event> = vec![GameEvent::PlayerAttacked {
                    aggregate_id: self.id.clone(),
                    attacker: player.clone(),
                }
                .into()];

                // Second event: the player who just scored a point might also have scored the goal and win the game.
                if player.points >= self.goal {
                    events.push(
                        GameEvent::GameEndedWithWinner {
                            aggregate_id: self.id.clone(),
                            winner: player.clone(),
                        }
                        .into(),
                    );
                }

                Ok(events)
            }
        }
    }

    fn apply(&mut self, event: &Self::Event) {
        match event {
            GameEvent::GameStarted {
                aggregate_id: _,
                player_1,
                player_2,
                goal,
            } => {
                // Game is started, we can populate the aggregate with the correct data.
                self.status = GameStatus::Playing;
                self.goal = *goal;
                self.player_1 = player_1.clone();
                self.player_2 = player_2.clone();
            }

            // Just sets the points on player, no other business logic here.
            GameEvent::PlayerAttacked {
                aggregate_id: _,
                attacker,
            } => {
                if self.player_1.id == attacker.id {
                    self.player_1.points += 1;
                } else {
                    self.player_2.points += 1;
                }
            }
            // We handle the end of the game separately.
            GameEvent::GameEndedWithWinner {
                aggregate_id: _,
                winner,
            } => self.status = GameStatus::Winner(winner.clone()),
        };
    }

    fn aggregate_id(&self) -> Self::Id {
        self.id.clone()
    }

    fn set_aggregate_id(&mut self, id: Self::Id) {
        self.id = id.clone();
    }
}

// Repository: a simple storage to project aggregate data so that it can be read/updated from the
// outside. It exposes two queries to read and updated a read-model. For demonstration purposes it
// has been kept very simple, but we might have many read models with more complex data structures.

#[derive(Default, Clone, Debug)]
pub struct InMemoryRepository {
    games: HashMap<String, GameModel>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        InMemoryRepository {
            games: HashMap::new(),
        }
    }

    pub async fn get_game(&self, id: String) -> Option<GameModel> {
        self.games.get(&id).cloned()
    }

    pub async fn update_game(&mut self, id: &str, read_model: GameModel) {
        self.games.insert(id.to_string(), read_model.clone());
    }
}

impl Repository for InMemoryRepository {}

#[derive(Clone)]
pub struct GetGameQuery {
    aggregate_id: String,
    repo: Arc<Mutex<InMemoryRepository>>,
}

impl GetGameQuery {
    pub fn new(aggregate_id: String, repo: Arc<Mutex<InMemoryRepository>>) -> Self {
        Self { aggregate_id, repo }
    }
}

#[async_trait]
impl Query for GetGameQuery {
    type Output = Result<Option<GameModel>, CqrsError>;

    async fn apply(&self) -> Self::Output {
        let result: Option<GameModel> = self
            .repo
            .lock()
            .await
            .get_game(self.aggregate_id.clone())
            .await;
        Ok(result)
    }
}

#[derive(Clone)]
pub struct AppQueries {}

#[async_trait]
impl QueriesRunner for AppQueries {}

// Read model: stores game data. This simple data structure might just fit into an SQL database
// table.

#[derive(Clone, Debug, PartialEq)]
pub struct GameModel {
    pub id: String,
    pub goal: u32,
    pub player_1: Player,
    pub player_2: Player,
    pub status: GameStatus,
}

// Here we define a Model Reader and its queries for a given read model.
// In theory, we could have many Model Readers, but I still need to test this behaviour.
#[derive(Clone)]
pub struct GameView {
    repo: Arc<Mutex<InMemoryRepository>>,
}

impl GameView {
    pub fn new(repo: Arc<Mutex<InMemoryRepository>>) -> Self {
        Self { repo }
    }

    pub fn repo(&self) -> Arc<Mutex<InMemoryRepository>> {
        self.repo.clone()
    }
}

#[async_trait]
impl ModelReader for GameView {
    type Repo = InMemoryRepository;
    type Model = GameModel;

    async fn update(&mut self, data: Self::Model) -> Result<(), CqrsError> {
        self.repo
            .lock()
            .await
            .update_game(&data.id, data.to_owned())
            .await;
        Ok(())
    }
}

// Consumer: it contains an instance of the repository, so that it can write updates on it when
// some event happens.

#[derive(Clone)]
pub struct CounterConsumer {
    game_model: GameView,
    queries: AppQueries,
}

impl CounterConsumer {
    pub fn new(repo: Arc<Mutex<InMemoryRepository>>) -> Self {
        Self {
            game_model: GameView::new(repo),
            queries: AppQueries {},
        }
    }
}

#[async_trait]
impl EventConsumer for CounterConsumer {
    async fn process(&mut self, evt: Event) {
        let event = evt.get_payload();

        match event {
            GameEvent::GameStarted {
                aggregate_id,
                player_1,
                player_2,
                goal,
            } => {
                let model = GameModel {
                    id: aggregate_id.clone(),
                    player_1: player_1.clone(),
                    player_2: player_2.clone(),
                    goal,
                    status: GameStatus::Playing,
                };
                _ = self.game_model.update(model).await;
            }
            GameEvent::PlayerAttacked {
                aggregate_id,
                attacker,
            } => {
                let q = GetGameQuery::new(aggregate_id, self.game_model.repo());
                if let Ok(Some(mut model)) = self.queries.run(q).await {
                    if model.player_1.id == attacker.id {
                        model.player_1.points += 1;
                    } else {
                        model.player_2.points += 1;
                    };
                    _ = self.game_model.update(model).await;
                }
            }
            GameEvent::GameEndedWithWinner {
                aggregate_id,
                winner,
            } => {
                let q = GetGameQuery::new(aggregate_id, self.game_model.repo());
                if let Ok(Some(mut model)) = self.queries.run(q).await {
                    model.status = GameStatus::Winner(winner.clone());
                    _ = self.game_model.update(model).await;
                }
            }
        }
    }
}

event_consumers_group! {
    GameEventConsumers {
        Counter => CounterConsumer,
    }
}

pub fn verify_game_result(
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

pub type GameDispatcher = SimpleDispatcher<GameState, InMemoryEventStore, GameEventConsumers>;
pub type GameSnapshotDispatcher = SnapshotDispatcher<
    GameState,
    InMemoryEventStore,
    InMemorySnapshotStore<GameState>,
    GameEventConsumers,
>;
