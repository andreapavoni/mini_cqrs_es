/// In this example there's a more complex use case by introducing a read model with a
/// repository that can read and write to a database (an in-memory one here) through event
/// consumer.
/// Implementation is very basic, but it has been used to explore and test a more realistic use
/// case. So, we have a Game, with 2 players, a score goal and an action that sets the points on
/// the player. The first player that reaches the score goal wins the game.
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use async_trait::async_trait;

use mini_cqrs::{Aggregate, CqrsError, Dispatcher, EventConsumer, EventStore, SimpleDispatcher};

// Event Store
struct InMemoryEventStore {
    events: HashMap<String, Vec<GameEvent>>,
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
    type Event = GameEvent;
    type AggregateId = String;

    async fn save_events(
        &mut self,
        aggregate_id: Self::AggregateId,
        events: &Vec<Self::Event>,
    ) -> Result<(), CqrsError> {
        if let Some(current_events) = self.events.get_mut(&aggregate_id) {
            current_events.extend(events.clone());
        } else {
            self.events.insert(aggregate_id.to_string(), events.clone());
        };
        Ok(())
    }

    async fn load_events(
        &self,
        aggregate_id: Self::AggregateId,
    ) -> Result<Vec<Self::Event>, CqrsError> {
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
#[derive(PartialEq)]
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
#[derive(Debug, PartialEq, Clone)]
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

// Aggregate: it's a more complex data structure with structs and enums as field values.
#[derive(Default, Clone, Debug, PartialEq)]
struct Player {
    pub id: String,
    pub points: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum GameStatus {
    Playing,
    Winner(Player),
}

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

    async fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, CqrsError> {
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
                player_1: player_1.clone(),
                player_2: player_2.clone(),
                goal: *goal,
            }]),
            GameCommand::AttackPlayer { attacker } => {
                let mut player = if self.player_1.id == attacker.id {
                    self.player_1.clone()
                } else {
                    self.player_2.clone()
                };
                // First event: a player attacks its opponent and increases its points.
                let mut events: Vec<GameEvent> = vec![GameEvent::PlayerAttacked {
                    aggregate_id: self.id.clone(),
                    attacker: player.clone(),
                }];

                // Second event: the player who just scored a point might also have scored the goal and win the game.
                if player.points + 1 >= self.goal {
                    player.points += 1;
                    events.push(GameEvent::GameEndedWithWinner {
                        aggregate_id: self.id.clone(),
                        winner: player.clone(),
                    });
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
    games: HashMap<String, GameReadModel>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        InMemoryRepository {
            games: HashMap::new(),
        }
    }

    pub async fn get_game(&self, id: String) -> Option<GameReadModel> {
        self.games.get(&id).cloned()
    }

    pub async fn update_game(&mut self, id: &str, read_model: GameReadModel) {
        self.games.insert(id.to_string(), read_model.clone());
    }
}

// Read model: stores game data. This simple data structure might just fit into an SQL database
// table.

#[derive(Clone, Debug, PartialEq)]
struct GameReadModel {
    goal: u32,
    player_1: Player,
    player_2: Player,
    status: GameStatus,
}

// Consumer: it contains an instance of the repository, so that it can write updates on it when
// some event happens.

struct GameEventConsumer {
    repo: Arc<Mutex<InMemoryRepository>>,
}

#[async_trait]
impl EventConsumer for GameEventConsumer {
    type Event = GameEvent;

    async fn process<'a>(&mut self, event: &'a Self::Event) {
        match event {
            GameEvent::GameStarted {
                aggregate_id,
                player_1,
                player_2,
                goal,
            } => {
                let model = GameReadModel {
                    player_1: player_1.clone(),
                    player_2: player_2.clone(),
                    goal: *goal,
                    status: GameStatus::Playing,
                };
                self.repo
                    .lock()
                    .await
                    .update_game(aggregate_id, model.to_owned())
                    .await;
            }
            GameEvent::PlayerAttacked {
                aggregate_id,
                attacker,
            } => {
                let mut model: GameReadModel = self
                    .repo
                    .lock()
                    .await
                    .get_game(aggregate_id.clone())
                    .await
                    .unwrap();

                if model.player_1.id == attacker.id {
                    model.player_1.points = attacker.points;
                } else {
                    model.player_2.points = attacker.points;
                };

                self.repo
                    .lock()
                    .await
                    .update_game(aggregate_id, model.to_owned())
                    .await;
            }
            GameEvent::GameEndedWithWinner {
                aggregate_id,
                winner,
            } => {
                let mut model: GameReadModel = self
                    .repo
                    .lock()
                    .await
                    .get_game(aggregate_id.clone())
                    .await
                    .unwrap();

                model.status = GameStatus::Winner(winner.clone());
                self.repo
                    .lock()
                    .await
                    .update_game(aggregate_id, model.to_owned())
                    .await;
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();
    let repo = Arc::new(Mutex::new(InMemoryRepository::new()));

    let consumers: Vec<Box<dyn EventConsumer<Event = GameEvent>>> =
        vec![Box::new(GameEventConsumer { repo: repo.clone() })];

    let mut dispatcher: SimpleDispatcher<GameState, InMemoryEventStore> =
        SimpleDispatcher::new(store, consumers);

    let player_1 = Player {
        id: "player_1".to_string(),
        points: 0,
    };
    let player_2 = Player {
        id: "player_2".to_string(),
        points: 0,
    };

    dispatcher
        .execute(
            "12345".to_string(),
            &GameCommand::StartGame {
                player_1: player_1.clone(),
                player_2: player_2.clone(),
                goal: 3,
            },
        )
        .await?;

    let read_model = repo
        .lock()
        .await
        .get_game("12345".to_string())
        .await
        .unwrap();

    assert_eq!(read_model.player_1.id, "player_1".to_string());
    assert_eq!(read_model.player_2.id, "player_2".to_string());
    assert_eq!(read_model.goal, 3);
    assert_eq!(read_model.status, GameStatus::Playing);

    let result = dispatcher
        .execute(
            "12345".to_string(),
            &GameCommand::AttackPlayer {
                attacker: player_1.clone(),
            },
        )
        .await?;

    assert_eq!(result.player_1.points, 1);
    assert_eq!(result.player_2.points, 0);
    assert_eq!(result.status, GameStatus::Playing);

    let result = dispatcher
        .execute(
            "12345".to_string(),
            &GameCommand::AttackPlayer {
                attacker: player_1.clone(),
            },
        )
        .await?;

    assert_eq!(result.player_1.points, 2);
    assert_eq!(result.player_2.points, 0);
    assert_eq!(result.status, GameStatus::Playing);

    let result = dispatcher
        .execute(
            "12345".to_string(),
            &GameCommand::AttackPlayer { attacker: player_2 },
        )
        .await?;

    assert_eq!(result.player_1.points, 2);
    assert_eq!(result.player_2.points, 1);
    assert_eq!(result.status, GameStatus::Playing);

    let result = dispatcher
        .execute(
            "12345".to_string(),
            &GameCommand::AttackPlayer {
                attacker: player_1.clone(),
            },
        )
        .await?;

    let winner = Player {
        id: "player_1".to_string(),
        points: 3,
    };

    assert_eq!(result.player_1.points, 3);
    assert_eq!(result.player_2.points, 1);
    assert_eq!(result.status, GameStatus::Winner(winner.clone()));

    let read_model = repo
        .lock()
        .await
        .get_game("12345".to_string())
        .await
        .unwrap();
    assert_eq!(read_model.status, GameStatus::Winner(winner));

    println!("Read model: {:?}", read_model);
    Ok(())
}
