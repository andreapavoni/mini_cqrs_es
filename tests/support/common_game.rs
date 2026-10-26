#![allow(dead_code)]
use super::read_store::read_error;
use mini_cqrs_es::{
    Aggregate, Command, CqrsError, EventPayload, Projection, Query, StoredEvent, toasty,
};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
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
            return Err(CqrsError::domain(format!(
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

impl EventPayload for GameEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::GameStarted { .. } => "GameStarted",
            Self::PlayerAttacked { .. } => "PlayerAttacked",
            Self::GameEndedWithWinner { .. } => "GameEndedWithWinner",
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
    player_1: Player,
    player_2: Player,
    status: GameStatus,
    goal: u32,
}

impl Default for GameAggregate {
    fn default() -> Self {
        Self {
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

    fn aggregate_type() -> &'static str {
        "example.game"
    }

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
}

#[derive(Debug, toasty::Model)]
#[table = "game_read_models"]
pub struct GameReadModel {
    #[key]
    pub id: String,
    pub goal: u32,
    pub player_1_id: String,
    pub player_1_points: u32,
    pub player_2_id: String,
    pub player_2_points: u32,
    pub winner_id: Option<String>,
    pub winner_points: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameModel {
    pub id: GameId,
    pub goal: u32,
    pub player_1: Player,
    pub player_2: Player,
    pub status: GameStatus,
}

#[derive(Clone)]
pub struct GetGameQuery {
    aggregate_id: GameId,
    database: toasty::Db,
}
impl GetGameQuery {
    pub fn new(aggregate_id: GameId, database: toasty::Db) -> Self {
        Self {
            aggregate_id,
            database,
        }
    }
}
impl Query for GetGameQuery {
    type Output = Result<Option<GameModel>, CqrsError>;
    async fn apply(&self) -> Self::Output {
        let mut database = self.database.clone();
        let row = GameReadModel::filter_by_id(self.aggregate_id.to_string())
            .first()
            .exec(&mut database)
            .await
            .map_err(read_error)?;
        Ok(row.map(|row| {
            let player_1 = Player {
                id: row.player_1_id,
                points: row.player_1_points,
            };
            let player_2 = Player {
                id: row.player_2_id,
                points: row.player_2_points,
            };
            let status = row.winner_id.map_or(GameStatus::Playing, |id| {
                GameStatus::Winner(Player {
                    id,
                    points: row.winner_points,
                })
            });
            GameModel {
                id: self.aggregate_id.clone(),
                goal: row.goal,
                player_1,
                player_2,
                status,
            }
        }))
    }
}

#[derive(Clone)]
pub struct GameProjection;

impl Projection for GameProjection {
    type Aggregate = GameAggregate;

    fn name() -> &'static str {
        "game"
    }

    async fn apply(
        &self,
        event: &StoredEvent,
        payload: GameEvent,
        executor: &mut dyn toasty::Executor,
    ) -> Result<(), CqrsError> {
        match payload {
            GameEvent::GameStarted {
                player_1,
                player_2,
                goal,
            } => {
                GameReadModel::upsert_by_id(&event.aggregate_id)
                    .goal(goal)
                    .player_1_id(player_1.id)
                    .player_1_points(player_1.points)
                    .player_2_id(player_2.id)
                    .player_2_points(player_2.points)
                    .winner_id(None::<String>)
                    .winner_points(0u32)
                    .exec(&mut *executor)
                    .await
                    .map_err(read_error)?;
            }
            GameEvent::PlayerAttacked { attacker } => {
                let mut row = GameReadModel::get_by_id(&mut *executor, &event.aggregate_id)
                    .await
                    .map_err(read_error)?;
                if row.player_1_id == attacker.id {
                    let points = row
                        .player_1_points
                        .checked_add(1)
                        .ok_or_else(|| CqrsError::EventStore("read-model score overflow".into()))?;
                    row.update()
                        .player_1_points(points)
                        .exec(&mut *executor)
                        .await
                        .map_err(read_error)?;
                } else {
                    let points = row
                        .player_2_points
                        .checked_add(1)
                        .ok_or_else(|| CqrsError::EventStore("read-model score overflow".into()))?;
                    row.update()
                        .player_2_points(points)
                        .exec(&mut *executor)
                        .await
                        .map_err(read_error)?;
                }
            }
            GameEvent::GameEndedWithWinner { winner } => {
                let mut row = GameReadModel::get_by_id(&mut *executor, &event.aggregate_id)
                    .await
                    .map_err(read_error)?;
                row.update()
                    .winner_id(Some(winner.id))
                    .winner_points(winner.points)
                    .exec(&mut *executor)
                    .await
                    .map_err(read_error)?;
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
