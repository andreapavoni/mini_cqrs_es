#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Game {}

pub struct AppQueries {}

impl Queries for AppQueries {
    fn run<A, B>(&self, query: A) -> B
    where
        A: QueryRunner + QueryRunner<Output = B>,
    {
        query.apply()
    }
}

pub trait Queries {
    fn run<A, B>(&self, query: A) -> B
    where
        A: QueryRunner + QueryRunner<Output = B>;
}

pub trait QueryRunner {
    // this associated type is used to simulate path dependent types.
    type Output;

    fn apply(&self) -> Self::Output;
}

pub struct GetGame {id: String}
impl QueryRunner for GetGame {
    type Output = Option<Game>;

    fn apply(&self) -> Self::Output {
        println!("GetGame -> {}", self.id);
        Some(Game {})
    }
}
pub struct ListGames {limit: u8}
impl QueryRunner for ListGames {
    type Output = Game;

    fn apply(&self) -> Self::Output {
        println!("ListGames -> {}", self.limit);
        Game {}
    }
}

fn main() {
    let queries = AppQueries {};

    let v1: Game = queries.run(GetGame { id: "1".to_string() }).unwrap();
    let v2: Game = queries.run(ListGames { limit: 1 });

    assert_eq!(Game {}, v1);
    assert_eq!(Game {}, v2);
}
