/// In this example there's a more complex use case by introducing a read model with a
/// repository that can read and write to a database (an in-memory one here) through event
/// consumer.
/// Implementation is very basic, but it has been used to explore and test a more realistic use
/// case. So, we have a Game, with 2 players, a score goal and an action that sets the points on
/// the player. The first player that reaches the score goal wins the game.
use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use mini_cqrs::{
    Aggregate, Cqrs, CqrsError, Event, EventConsumer, EventPayload, EventStore, ModelReader, Query,
    Repository, SimpleDispatcher, act_as_event,
};

// Event Store
struct InMemoryEventStore {
    events: HashMap<String, Vec<Event>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        InMemoryEventStore {
            events: HashMap::new(),
        }
    }
}

#[async_trait]
impl EventStore for InMemoryEventStore {
    type AggregateId = String;

    async fn save_events(
        &mut self,
        aggregate_id: Self::AggregateId,
        events: &[Event],
    ) -> Result<(), CqrsError> {
        if let Some(current_events) = self.events.get_mut(&aggregate_id) {
            current_events.extend(events.to_vec());
        } else {
            self.events
                .insert(aggregate_id.to_string(), events.into());
        };
        Ok(())
    }

    async fn load_events(
        &self,
        aggregate_id: Self::AggregateId,
    ) -> Result<Vec<Event>, CqrsError> {
        if let Some(events) = self.events.get(&aggregate_id) {
            Ok(events.to_vec())
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
enum GameCommand {
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
enum GameEvent {
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

act_as_event!(GameEvent);

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
struct Player {
    pub id: String,
    pub points: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum GameStatus {
    Playing,
    Winner(Player),
}

// Game aggregate
#[derive(Clone, Debug, PartialEq)]
struct GameState {
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
            } => Ok(vec![GameEvent::GameStarted { aggregate_id: self.id.clone(), player_1, player_2, goal, }.into()]),
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
                }.into()];

                // Second event: the player who just scored a point might also have scored the goal and win the game.
                if player.points >= self.goal {
                    events.push(GameEvent::GameEndedWithWinner {
                        aggregate_id: self.id.clone(),
                        winner: player.clone(),
                    }.into());
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
struct InMemoryRepository {
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
enum GameQuery {
    GetGame(String),
    // AllGames,
}

#[derive(Clone)]
struct GetGameQuery {
    aggregate_id: String,
}

#[async_trait]
impl Query for GetGameQuery {
    type Output = GameModel;
    type Repo = InMemoryRepository;

    async fn run(&self, repo: Self::Repo) -> Result<Self::Output, CqrsError> {
        let result: Option<Self::Output> = repo.get_game(self.aggregate_id.clone()).await;
        Ok(result.unwrap())
    }
}

// Read model: stores game data. This simple data structure might just fit into an SQL database
// table.

#[derive(Clone, Debug, PartialEq)]
struct GameModel {
    id: String,
    goal: u32,
    player_1: Player,
    player_2: Player,
    status: GameStatus,
}

// Here we define a Model Reader and its queries for a given read model.
// In theory, we could have many Model Readers, but I still need to test this behaviour.
#[derive(Clone)]
struct GameView {
    repo: Arc<Mutex<InMemoryRepository>>,
}

impl GameView {
    fn new(repo: Arc<Mutex<InMemoryRepository>>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl ModelReader for GameView {
    type Repo = InMemoryRepository;
    type Query = GameQuery;
    type Output = GameModel;

    async fn query(&self, query: Self::Query) -> Result<Self::Output, CqrsError> {
        match query {
            GameQuery::GetGame(id) => {
                let qr = GetGameQuery { aggregate_id: id };
                qr.run(self.repo.lock().await.clone()).await
            } // GameQuery::AllGames => {}
        }
    }

    async fn update(&mut self, data: Self::Output) -> Result<(), CqrsError> {
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

struct GameEventConsumer {
    game_model: GameView,
}

impl GameEventConsumer {
    fn new(repo: Arc<Mutex<InMemoryRepository>>) -> Self {
        Self {
            game_model: GameView::new(repo),
        }
    }
}

#[async_trait]
impl EventConsumer for GameEventConsumer {

    async fn process<'a>(&mut self, evt: &'a Event) {
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
                    goal: goal,
                    status: GameStatus::Playing,
                };
                _ = self.game_model.update(model).await;
            }
            GameEvent::PlayerAttacked {
                aggregate_id,
                attacker,
            } => {
                let mut model = self
                    .game_model
                    .query(GameQuery::GetGame(aggregate_id.clone()))
                    .await
                    .unwrap();
                if model.player_1.id == attacker.id {
                    model.player_1.points += 1;
                } else {
                    model.player_2.points += 1;
                };
                _ = self.game_model.update(model).await;
            }
            GameEvent::GameEndedWithWinner {
                aggregate_id,
                winner,
            } => {
                let mut model = self
                    .game_model
                    .query(GameQuery::GetGame(aggregate_id.clone()))
                    .await
                    .unwrap();
                model.status = GameStatus::Winner(winner.clone());
                _ = self.game_model.update(model).await;
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();
    let repo = Arc::new(Mutex::new(InMemoryRepository::new()));

    let consumers: Vec<Box<dyn EventConsumer>> =
        vec![Box::new(GameEventConsumer::new(repo.clone()))];

    let dispatcher: SimpleDispatcher<GameState, InMemoryEventStore> =
        SimpleDispatcher::new(store, consumers);

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
