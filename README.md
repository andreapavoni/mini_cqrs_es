# MiniCQRS/ES - Simplifying CQRS for Rust

Simple, minimal, opinionated micro-framework to implement CQRS/ES in Rust. There are already a lot of opinionated libraries to do that but, in fact, I didn't agree with their
opinions and I've seen an opportunity to improve my knowledge and practice with Rust.

This lightweight library offers the necessary components abstractions and minimal glue to streamline the implementation of CQRS architecture, making it a breeze to manage your application's data flow.

## Features

- It's a microframework, so _micro_ that you mostly get only the _frame_, the _work_ is on you:
  - you write your own implementations;
  - you choose the libraries, storage engines, and other external tools;
  - you can pick only the pieces you need (aggregate, command, events, queries, etc...).

- Fully async — uses native `async fn` in traits (no `async_trait` dependency).
- All trait methods take `&self`, enabling safe concurrent command execution.
- Built-in optimistic concurrency control via event versioning.
- `anyhow` re-exported so you don't need a separate dependency.

### Architecture

- **Aggregates:** Define your domain entities as aggregates, handle state changes, and apply events with straightforward traits.

- **Commands:** Implement custom commands that return domain events directly. The framework wraps them and handles versioning.

- **Event Store:** Store and retrieve events efficiently with optimistic concurrency built in. Implement the trait against any storage backend (SQLite, Postgres, Redis, etc.).

- **Snapshot Store:** Optionally use snapshots to speed up aggregate state recovery from long event streams.

- **Event Consumers:** Process events through a composable consumer pipeline built with a simple builder pattern.

- **Queries:** Implement custom queries to retrieve data from your read models.

- **Error Handling:** Structured `CqrsError` enum with variants for domain errors, conflicts, serialization failures, and more. `anyhow` is re-exported for ergonomic application-level error handling.

## Status

The library is somewhat usable for experimenting on some real use cases, but I wouldn't recommend
it for production use yet. The API is still **under active development — breaking changes can be introduced**.

## Installation

Run the following Cargo command in your project directory:

```sh
cargo add mini_cqrs_es
```

Or add the following line to your `Cargo.toml`:

```toml
[dependencies]
mini_cqrs_es = "0.9.0"
```

## Usage

Once you have added the library to your project's dependencies, import it:

```rust
use mini_cqrs_es::*;
// or selectively:
// use mini_cqrs_es::{Aggregate, Command, SimpleCqrs, EventConsumers, ...};
```

Being made almost entirely of traits, MiniCQRS/ES is flexible but requires some boilerplate.
Check out the [examples directory](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples) for complete implementations.

When you have implemented the various traits, you can wire up your CQRS architecture.

Here's a snippet inspired by the [game example](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples/game.rs):

```rust
// An implementation of the EventStore trait (backed by any storage engine you choose)
let event_store = InMemoryEventStore::new();
// An implementation of the SnapshotStore trait (optional — for faster aggregate loading)
let snapshot_store = InMemorySnapshotStore::<GameAggregate>::new();
// SnapshotAggregateManager is provided by MiniCQRS/ES
let aggregate_manager = SnapshotAggregateManager::new(snapshot_store);

// Build a consumer pipeline with the builder pattern
let repo = Arc::new(Mutex::new(InMemoryRepository::new()));
let consumers = EventConsumers::new()
    .with(GameMainConsumer::new(repo.clone()))
    .with(PrintEventConsumer {});

// SimpleCqrs is provided by MiniCQRS/ES and wires everything together
// Note: takes &self — safe to share across async tasks via Arc
let cqrs = SimpleCqrs::new(aggregate_manager, event_store, consumers);

// Build a command and execute it
let aggregate_id = Uuid::new_v4();
let cmd = CmdStartGame { player_1, player_2, goal: 3 };
cqrs.execute(aggregate_id, &cmd).await?;

// Query the read model
let query = GetGameQuery::new(aggregate_id, repo.clone());
let result = cqrs.query(&query).await?.unwrap();
```

Commands return domain-specific event types directly — the framework handles wrapping them into `Event` structs with versioning:

```rust
impl Command for CmdStartGame {
    type Aggregate = GameAggregate;

    async fn handle(&self, aggregate: &GameAggregate) -> Result<Vec<GameEvent>, CqrsError> {
        Ok(vec![GameEvent::GameStarted {
            player_1: self.player_1.clone(),
            player_2: self.player_2.clone(),
            goal: self.goal,
        }])
    }
}
```

Event payloads no longer need to carry an `aggregate_id` field — the framework injects it automatically.

For application-level error handling, `anyhow` is re-exported:

```rust
async fn main() -> mini_cqrs_es::anyhow::Result<()> {
    // ...
}
```

Please note that, even if there are some ready to use structs (like `SimpleCqrs`, `SimpleAggregateManager`, and `SnapshotAggregateManager`), everything is an implementation of some trait. The `InMemory*` types in the [game example](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples/game.rs) simulate storage. In real use cases you will build wrappers around your database client or other storage solution — see the [hotel example](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples/hotel.rs) for a complete SQLite implementation using `sqlx`.

## Documentation

Code is documented and being improved continuously. For complete implementations, check the
[examples](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples):

- **[game](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples/game.rs)** — in-memory stores with snapshots
- **[hotel](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples/hotel.rs)** — SQLite event store via `sqlx`, with tests covering the full domain lifecycle and event replay consistency

## Testing

The hotel example includes integration tests that exercise the full CQRS/ES stack against a real SQLite database:

```sh
cargo test --example hotel
cargo run --example hotel
cargo run --example game
```

## Contributing

If you find any bugs or have any suggestions, please [open an issue](https://github.com/andreapavoni/mini_cqrs_es/issues).

## License

This project is open-source and available under the [MIT License](LICENSE).
