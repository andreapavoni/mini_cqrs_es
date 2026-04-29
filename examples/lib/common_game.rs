#![allow(dead_code)]

// Common code shared in the examples to avoid repetitions and focus on the core concepts

use std::{
    collections::HashMap,
    fmt,
    str::FromStr,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use mini_cqrs_es::{
    Aggregate, AggregateSnapshot, Command, CqrsError, EventConsumer, EventPayload, Query,
    Repository, SnapshotStore, StoredEvent,
};

#[path = "common.rs"]
mod common;
pub use common::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameId(String);

impl GameId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for GameId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

// Snapshot Store
pub struct InMemorySnapshotStore<T>
where
    T: Aggregate,
{
    snapshots: Mutex<HashMap<String, AggregateSnapshot<T>>>,
}

impl<T> InMemorySnapshotStore<T>
where
    T: Aggregate,
{
    pub fn new() -> Self {
        InMemorySnapshotStore {
            snapshots: Mutex::new(HashMap::new()),
        }
    }
}

impl<A> SnapshotStore for InMemorySnapshotStore<A>
where
    A: Aggregate,
{
    async fn save_snapshot<T>(&self, snapshot: AggregateSnapshot<T>) -> Result<(), CqrsError>
    where
        T: Aggregate,
    {
        let aggregate = snapshot.get_payload::<A>()?;
        let snapshot = AggregateSnapshot::new(&aggregate, Some(snapshot.version))?;
        let aggregate_id = snapshot.aggregate_id.to_string();

        let mut store = self.snapshots.lock().unwrap();
        store.insert(aggregate_id, snapshot);
        Ok(())
    }

    async fn load_snapshot<T>(
        &self,
        aggregate_id: &T::Id,
    ) -> Result<AggregateSnapshot<T>, CqrsError>
    where
        T: Aggregate,
    {
        let store = self.snapshots.lock().unwrap();
        if let Some(snapshot) = store.get(&aggregate_id.to_string()) {
            let aggregate = snapshot.get_payload::<T>()?;
            Ok(AggregateSnapshot::new(&aggregate, Some(snapshot.version))?)
        } else {
            Err(CqrsError::SnapshotStore(format!(
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

impl Command for CmdStartGame {
    type Aggregate = GameAggregate;

    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<GameEvent>, CqrsError> {
        if aggregate.status != GameStatus::Playing {
            return Err(CqrsError::Domain(format!(
                "Game is already finished with state {:?}",
                aggregate.status
            )));
        }

        Ok(vec![GameEvent::GameStarted {
            player_1: self.player_1.clone(),
            player_2: self.player_2.clone(),
            goal: self.goal,
        }])
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct CmdAttackPlayer {
    pub attacker: Player,
}

impl Command for CmdAttackPlayer {
    type Aggregate = GameAggregate;

    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<GameEvent>, CqrsError> {
        let mut player = if aggregate.player_1.id == self.attacker.id {
            aggregate.player_1.clone()
        } else {
            aggregate.player_2.clone()
        };

        player.points += 1;

        let mut events = vec![GameEvent::PlayerAttacked {
            attacker: player.clone(),
        }];

        if player.points >= aggregate.goal {
            events.push(GameEvent::GameEndedWithWinner {
                winner: player.clone(),
            });
        }

        Ok(events)
    }
}

// Events: the outcomes of the above commands, including the end of the game with a winner.
// Note: aggregate_id is NOT needed in event variants — the framework handles it.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum GameEvent {
    GameStarted {
        player_1: Player,
        player_2: Player,
        goal: u32,
    },
    PlayerAttacked {
        attacker: Player,
    },
    GameEndedWithWinner {
        winner: Player,
    },
}

impl fmt::Display for GameEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GameEvent::GameStarted { .. } => write!(f, "GameStarted"),
            GameEvent::PlayerAttacked { .. } => write!(f, "PlayerAttacked"),
            GameEvent::GameEndedWithWinner { .. } => write!(f, "GameEndedWithWinner"),
        }
    }
}

impl EventPayload for GameEvent {}

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
    id: GameId,
    version: u64,
    player_1: Player,
    player_2: Player,
    status: GameStatus,
    goal: u32,
}

impl Default for GameAggregate {
    fn default() -> Self {
        Self {
            id: GameId::new(uuid::Uuid::new_v4().to_string()),
            version: 0,
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

impl Aggregate for GameAggregate {
    type Id = GameId;
    type Event = GameEvent;

    async fn apply(&mut self, event: &Self::Event) {
        match event {
            GameEvent::GameStarted {
                player_1,
                player_2,
                goal,
            } => {
                self.status = GameStatus::Playing;
                self.goal = *goal;
                self.player_1 = player_1.clone();
                self.player_2 = player_2.clone();
            }

            GameEvent::PlayerAttacked { attacker } => {
                if self.player_1.id == attacker.id {
                    self.player_1.points += 1;
                } else {
                    self.player_2.points += 1;
                }
            }

            GameEvent::GameEndedWithWinner { winner } => {
                self.status = GameStatus::Winner(winner.clone());
            }
        };
    }

    fn aggregate_id(&self) -> Self::Id {
        self.id.clone()
    }

    fn set_aggregate_id(&mut self, id: Self::Id) {
        self.id = id;
    }

    fn version(&self) -> u64 {
        self.version
    }

    fn set_version(&mut self, version: u64) {
        self.version = version;
    }
}

// Repository: a simple storage to project aggregate data so that it can be read/updated from the
// outside.

#[derive(Default, Clone, Debug)]
pub struct InMemoryRepository {
    games: HashMap<GameId, GameModel>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        InMemoryRepository {
            games: HashMap::new(),
        }
    }

    pub fn get_game(&self, id: &GameId) -> Option<GameModel> {
        self.games.get(id).cloned()
    }

    pub fn update_game(&mut self, id: GameId, read_model: GameModel) {
        self.games.insert(id, read_model);
    }
}

impl Repository for InMemoryRepository {}

#[derive(Clone)]
pub struct GetGameQuery {
    aggregate_id: GameId,
    repo: Arc<Mutex<InMemoryRepository>>,
}

impl GetGameQuery {
    pub fn new(aggregate_id: GameId, repo: Arc<Mutex<InMemoryRepository>>) -> Self {
        Self { aggregate_id, repo }
    }
}

impl Query for GetGameQuery {
    type Output = Result<Option<GameModel>, CqrsError>;

    async fn apply(&self) -> Self::Output {
        let repo = self.repo.lock().unwrap();
        Ok(repo.get_game(&self.aggregate_id))
    }
}

// Read model: stores game data.

#[derive(Clone, Debug, PartialEq)]
pub struct GameModel {
    pub id: GameId,
    pub goal: u32,
    pub player_1: Player,
    pub player_2: Player,
    pub status: GameStatus,
}

// Consumer: it contains an instance of the repository, so that it can write updates on it when
// some event happens.

#[derive(Clone)]
pub struct GameMainConsumer {
    repo: Arc<Mutex<InMemoryRepository>>,
}

impl GameMainConsumer {
    pub fn new(repo: Arc<Mutex<InMemoryRepository>>) -> Self {
        Self { repo }
    }
}

impl EventConsumer for GameMainConsumer {
    async fn process(&self, evt: &StoredEvent) -> Result<(), CqrsError> {
        let event = evt.get_payload::<GameEvent>()?;

        match event {
            GameEvent::GameStarted {
                player_1,
                player_2,
                goal,
            } => {
                let model = GameModel {
                    id: GameId::new(evt.aggregate_id.clone()),
                    player_1: player_1.clone(),
                    player_2: player_2.clone(),
                    goal,
                    status: GameStatus::Playing,
                };
                self.repo
                    .lock()
                    .unwrap()
                    .update_game(GameId::new(evt.aggregate_id.clone()), model);
            }
            GameEvent::PlayerAttacked { attacker } => {
                let mut repo = self.repo.lock().unwrap();
                if let Some(mut model) = repo.get_game(&GameId::new(evt.aggregate_id.clone())) {
                    if model.player_1.id == attacker.id {
                        model.player_1.points += 1;
                    } else {
                        model.player_2.points += 1;
                    }
                    repo.update_game(GameId::new(evt.aggregate_id.clone()), model);
                }
            }
            GameEvent::GameEndedWithWinner { winner } => {
                let mut repo = self.repo.lock().unwrap();
                if let Some(mut model) = repo.get_game(&GameId::new(evt.aggregate_id.clone())) {
                    model.status = GameStatus::Winner(winner.clone());
                    repo.update_game(GameId::new(evt.aggregate_id.clone()), model);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PrintEventConsumer {}

impl EventConsumer for PrintEventConsumer {
    async fn process(&self, event: &StoredEvent) -> Result<(), CqrsError> {
        let payload = event.get_payload::<GameEvent>()?;

        match payload {
            GameEvent::GameStarted {
                player_1,
                player_2,
                goal,
            } => {
                println!(
                    "LOG: Game started. (Player 1: `{}`, Player 2: `{}`, Goal: `{}`)",
                    player_1.id, player_2.id, goal
                )
            }
            GameEvent::PlayerAttacked { attacker } => {
                println!("LOG: {} has attacked his opponent", attacker.id)
            }
            GameEvent::GameEndedWithWinner { winner } => {
                println!("LOG: Game ended with winner: {}", winner.id)
            }
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::GameId;
    use std::str::FromStr;

    #[test]
    fn test_game_id_display_fromstr_roundtrip() {
        let id = GameId::new("game-123");
        let rendered = id.to_string();
        let parsed = GameId::from_str(&rendered).unwrap();
        assert_eq!(parsed, id);
    }
}
