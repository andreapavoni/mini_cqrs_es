# MiniCQRS/ES - Simplifying CQRS for Rust

Simple, minimal, opinionated micro-framework to implement CQRS/ES in Rust. There are already a lot of opinionated libraries to do that but, in fact, I didn't agree with their
opinions and I've seen an opportunity to improve my knowledge and practice with Rust.

This lightweight library offers the necessary components abstractions and minimal glue to streamline the implementation of CQRS architecture, making it a breeze to manage your application's data flow.

## Features

- High flexibility, isolation and extensibility by implementing traits. This means that:

  - you need to write your own implementations;
  - you can pick only the traits you need;
  - you have full control over the architecture;

- Almost everything is async using Tokio runtime.

### Architecture

- **Aggregates:** Define your domain entities as aggregates, handle state changes, and apply events with straightforward traits.

- **Commands:** Implement custom commands to change the state of your aggregates with minimal effort.

- **Event Stores:** Store and retrieve your events efficiently for seamless event-sourcing.

- **Snapshot Support:** Optionally use snapshots to speed up aggregate state recovery.

- **Event Handling:** Easily manage and consume events generated by your aggregates, to execute actions and real-time updates.

- **Queries:** Implement custom queries to retrieve data from your read models.

## Installation

To use MiniCQRS/ES, add it to your `Cargo.toml`. This library hasn't been published on crates.io yet.

```toml
[dependencies]
mini_cqrs_es = { git = "https://github.com/andreapavoni/mini_cqrs_es" }
```

## Usage

Once you have added the library to your project's dependencies, you can start using it by importing the library:

```
use mini_cqrs_es::*;
// or importing single components:
// use mini_cqrs_es::{Aggregate, etc...};
```

Being made almost entirely of traits, MiniCQRS/ES is very flexible but, of course,
it also requires some boilerplate. For this reason, you can check out the
[examples directory](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples) for more details.

When you have implemented the various traits from the library, you can finally build your CQRS architecture.

Here's a snippet extracted from the [game example](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples/game.rs):

```rust
// Setup components
let store = InMemoryEventStore::new();
let repo = Arc::new(Mutex::new(InMemoryRepository::new()));
let snapshot_store = InMemorySnapshotStore::<GameStateAggregate>::new();
let consumers = vec![GameEventConsumers::Counter(CounterConsumer::new(
    repo.clone(),
))];
let aggregate_manager = SnapshotAggregateManager::new(snapshot_store);

// Setup a Cqrs instance
let mut cqrs = Cqrs::new(aggregate_manager, store, consumers);

// Init a command with some arguments
let player_1 = Player { id: "player_1".to_string(), points: 0, };
let player_2 = Player { id: "player_2".to_string(), points: 0, };
let main_id = Uuid::new_v4();
let start_cmd = CmdStartGame {
    player_1: player_1.clone(),
    player_2: player_2.clone(),
    goal: 3,
};

// Execute the command
cqrs.execute(main_id, &start_cmd).await?;

// Query the read model
let q = GetGameQuery::new(main_id, repo.clone());
let result = cqrs.query(&q).await?.unwrap();
```

## Documentation

Code is documented and is being improved. For real working examples you can check the
[examples](https://github.com/andreapavoni/mini_cqrs_es/tree/master/examples).

## Contributing

If you find any bugs or have any suggestions, please [open an issue](https://github.com/andreapavoni/mini_cqrs_es/issues).

## License

This project is open-source and available under the [MIT License](LICENSE).

## Status

The project is somewhat usable for some real use cases, but I wouldn't recommend
it for production use.
