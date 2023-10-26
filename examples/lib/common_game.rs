#![allow(dead_code)]

// Common code shared in the examples to avoid repetitions and focus on the core concepts

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use mini_cqrs_es::{
    make_event_consumers_group, wrap_event, Aggregate, AggregateSnapshot, Command, CqrsError,
    Event, EventConsumer, EventConsumersGroup, EventPayload, ModelReader, QueriesRunner, Query,
    Repository, SnapshotStore, Uuid,
};

#[path = "common.rs"]
mod common;
pub use common::*;

#[derive(Clone, Debug)]
pub struct InMemorySnapshotStore<T>
where
    T: Aggregate,
{
    snapshots: HashMap<Uuid, AggregateSnapshot<T>>,
}

impl<T> InMemorySnapshotStore<T>
where
    T: Aggregate,
{
    pub fn new() -> Self {
        InMemorySnapshotStore {
            snapshots: HashMap::new(),
        }
    }
}

#[async_trait]
impl<A> SnapshotStore for InMemorySnapshotStore<A>
where
    A: Aggregate,
{
    async fn save_snapshot<T>(&mut self, snapshot: AggregateSnapshot<T>) -> Result<(), CqrsError>
    where
        T: Aggregate + Clone,
    {
        let aggregate = snapshot.get_payload::<A>();
        let snapshot = AggregateSnapshot::new(&aggregate, None);

        self.snapshots
            .insert(snapshot.clone().aggregate_id, snapshot);
        Ok(())
    }

    async fn load_snapshot<T>(&self, aggregate_id: Uuid) -> Result<AggregateSnapshot<T>, CqrsError>
    where
        T: Aggregate + Clone,
    {
        if let Some(snapshot) = self.snapshots.get(&aggregate_id) {
            let aggregate = snapshot.get_payload::<T>();

            Ok(AggregateSnapshot::new(&aggregate, None))
        } else {
            Err(CqrsError::new(format!(
                "No snapshot for aggregate id `{}`",
                aggregate_id
            )))
        }
    }
}

// Commands: for demonstration purposes, we can only start the game or attack the opponent.
#[derive(PartialEq, Clone, Debug)]
pub struct CmdStartGame {
    pub player_1: Player,
    pub player_2: Player,
    pub goal: u32,
}

#[async_trait]
impl Command for CmdStartGame {
    type Aggregate = GameAggregate;

    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<Event>, CqrsError> {
        if aggregate.status != GameStatus::Playing {
            return Err(CqrsError::new(format!(
                "Game is already finished with state {:?}",
                aggregate.status
            )));
        }

        let res = vec![GameEvent::GameStarted {
            aggregate_id: aggregate.aggregate_id(),
            player_1: self.player_1.clone(),
            player_2: self.player_2.clone(),
            goal: self.goal,
        }
        .into()];

        Ok(res)
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct CmdAttackPlayer {
    pub attacker: Player,
}

#[async_trait]
impl Command for CmdAttackPlayer {
    type Aggregate = GameAggregate;

    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<Event>, CqrsError> {
        let mut player = if aggregate.player_1.id == self.attacker.id {
            aggregate.player_1.clone()
        } else {
            aggregate.player_2.clone()
        };

        player.points += 1;

        // First event: a player attacks its opponent and increases its points.
        let mut events: Vec<Event> = vec![GameEvent::PlayerAttacked {
            aggregate_id: aggregate.aggregate_id(),
            attacker: player.clone(),
        }
        .into()]
        .clone();

        // Second event: the player who just scored a point might also have scored the goal and win the game.
        if player.points >= aggregate.goal {
            events.push(
                GameEvent::GameEndedWithWinner {
                    aggregate_id: aggregate.aggregate_id(),
                    winner: player.clone(),
                }
                .into(),
            );
        }

        Ok(events)
    }
}

// Events: the outcomes of the above commands, including the end of the game with a winner.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    GameStarted {
        aggregate_id: Uuid,
        player_1: Player,
        player_2: Player,
        goal: u32,
    },
    PlayerAttacked {
        aggregate_id: Uuid,
        attacker: Player,
    },
    GameEndedWithWinner {
        aggregate_id: Uuid,
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
    fn aggregate_id(&self) -> Uuid {
        match self {
            GameEvent::GameStarted { aggregate_id, .. } => *aggregate_id,
            GameEvent::PlayerAttacked { aggregate_id, .. } => *aggregate_id,
            GameEvent::GameEndedWithWinner { aggregate_id, .. } => *aggregate_id,
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
pub struct GameAggregate {
    id: Uuid,
    player_1: Player,
    player_2: Player,
    status: GameStatus,
    goal: u32,
}

// We don't care about the default starting values, any placeholder will be ok, because the real
// data will be loaded/stored when the game is started.
impl Default for GameAggregate {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
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
impl Aggregate for GameAggregate {
    type Event = GameEvent;

    async fn apply(&mut self, event: &Self::Event) {
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

    fn aggregate_id(&self) -> Uuid {
        self.id
    }

    fn set_aggregate_id(&mut self, id: Uuid) {
        self.id = id;
    }
}

// Repository: a simple storage to project aggregate data so that it can be read/updated from the
// outside. It exposes two queries to read and updated a read-model. For demonstration purposes it
// has been kept very simple, but we might have many read models with more complex data structures.

#[derive(Default, Clone, Debug)]
pub struct InMemoryRepository {
    games: HashMap<Uuid, GameModel>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        InMemoryRepository {
            games: HashMap::new(),
        }
    }

    pub async fn get_game(&self, id: Uuid) -> Option<GameModel> {
        self.games.get(&id).cloned()
    }

    pub async fn update_game(&mut self, id: Uuid, read_model: GameModel) {
        self.games.insert(id, read_model.clone());
    }
}

impl Repository for InMemoryRepository {}

#[derive(Clone)]
pub struct GetGameQuery {
    aggregate_id: Uuid,
    repo: Arc<Mutex<InMemoryRepository>>,
}

impl GetGameQuery {
    pub fn new(aggregate_id: Uuid, repo: Arc<Mutex<InMemoryRepository>>) -> Self {
        Self { aggregate_id, repo }
    }
}

#[async_trait]
impl Query for GetGameQuery {
    type Output = Result<Option<GameModel>, CqrsError>;

    async fn apply(&self) -> Self::Output {
        let result: Option<GameModel> = self.repo.lock().await.get_game(self.aggregate_id).await;

        Ok(result)
    }
}

// Read model: stores game data. This simple data structure might just fit into an SQL database
// table.

#[derive(Clone, Debug, PartialEq)]
pub struct GameModel {
    pub id: Uuid,
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
            .update_game(data.id, data.to_owned())
            .await;
        Ok(())
    }
}

// Consumer: it contains an instance of the repository, so that it can write updates on it when
// some event happens.

#[derive(Clone)]
pub struct GameMainConsumer {
    game_model: GameView,
}

impl GameMainConsumer {
    pub fn new(repo: Arc<Mutex<InMemoryRepository>>) -> Self {
        Self {
            game_model: GameView::new(repo),
        }
    }
}

impl QueriesRunner for GameMainConsumer {}

#[async_trait]
impl EventConsumer for GameMainConsumer {
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
                    id: aggregate_id,
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
                if let Ok(Some(mut model)) = self.query(&q).await {
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
                if let Ok(Some(mut model)) = self.query(&q).await {
                    model.status = GameStatus::Winner(winner.clone());
                    _ = self.game_model.update(model).await;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrintEventConsumer {}

#[async_trait]
impl EventConsumer for PrintEventConsumer {
    async fn process(&mut self, event: Event) {
        let payload: GameEvent = event.get_payload();

        match payload {
            GameEvent::GameStarted {
                aggregate_id: _,
                player_1,
                player_2,
                goal,
            } => {
                println!(
                    "LOG: Game started. (Player 1: `{}`, Player 2: `{}`, Goal: `{}`)",
                    player_1.id, player_2.id, goal
                )
            }
            GameEvent::PlayerAttacked {
                aggregate_id: _,
                attacker,
            } => {
                println!("LOG: {} has attacked his opponent", attacker.id)
            }
            GameEvent::GameEndedWithWinner {
                aggregate_id: _,
                winner,
            } => {
                println!("LOG: Game ended with winner: {}", winner.id)
            }
        }
    }
}

make_event_consumers_group! {
    GameEventConsumersGroup {
        main: GameMainConsumer,
        print: PrintEventConsumer,
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
