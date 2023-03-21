/// Another example with a more complex use case.
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

// Commands
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

// Events
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

// Aggregate
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
        if self.status != GameStatus::Playing {
            return Err(CqrsError::new(format!(
                "Game is already finished with state {:?}",
                self.status
            )));
        }

        match command {
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
                let player = if self.player_1.id == attacker.id {
                    self.player_1.clone()
                } else {
                    self.player_2.clone()
                };
                let mut events: Vec<GameEvent> = vec![GameEvent::PlayerAttacked {
                    aggregate_id: self.id.clone(),
                    attacker: player.clone(),
                }];

                if player.points + 1 >= self.goal {
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
                self.status = GameStatus::Playing;
                self.goal = *goal;
                self.player_1 = player_1.clone();
                self.player_2 = player_2.clone();
            }
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

// Repository
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

// Read model
#[derive(Clone, Debug, PartialEq)]
struct GameReadModel {
    goal: u32,
    player_1: Player,
    player_2: Player,
    status: GameStatus,
}

impl GameReadModel {}

// Consumer
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
                    model.player_1.points += 1;
                } else {
                    model.player_2.points += 1;
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

    dispatcher
        .execute(
            "12345".to_string(),
            &GameCommand::AttackPlayer {
                attacker: player_1.clone(),
            },
        )
        .await?;
    dispatcher
        .execute(
            "12345".to_string(),
            &GameCommand::AttackPlayer {
                attacker: player_1.clone(),
            },
        )
        .await?;
    dispatcher
        .execute(
            "12345".to_string(),
            &GameCommand::AttackPlayer { attacker: player_2 },
        )
        .await?;

    dispatcher
        .execute(
            "12345".to_string(),
            &GameCommand::AttackPlayer {
                attacker: player_1.clone(),
            },
        )
        .await?;

    let read_model = repo
        .lock()
        .await
        .get_game("12345".to_string())
        .await
        .unwrap();

    println!("Read model: {:?}", read_model);

    Ok(())
}
