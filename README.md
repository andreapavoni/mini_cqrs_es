# Mini CQRS

Simple, minimal, opinionated micro-framework to implement CQRS/ES in Rust. There are already
a lot of opinionated libraries to do that but, in fact, I didn't agree with their 
opinions and I've seen an opportunity to improve my knowledge and practice with Rust.

The main goals of this project are:

- Keep things simple and easy to understand and use
- Provide Rust traits as a guideline to implement the necessary building blocks
- Leave as much implementation choices as possible to the developer
- Provide some minimal glue code to assemble the pieces

**The project is somewhat usable for some real use cases, but I wouldn't recommend
it for production use. Not yet.**

## Features

Mini CQRS is a set of traits to implement CQRS/ES in Rust. Macros and default trait
implementations are also available where possible.

- Define aggregates for modeling your domain logic.
- Store and retrieve events on any implementation of the event store trait.
- Dispatch commands and process events with ease.
- Implement queries on read models for efficient data retrieval.
- Handle errors gracefully with provided error types and utilities.
- Almost everything is async using Tokio runtime (see Roadmap).

## Installation

To use Mini CQRS, add it to your `Cargo.toml`. This library hasn't been published on crates.io yet.

```toml
[dependencies]
mini_cqrs = [git = "https://github.com/andreapavoni/mini_cqrs"]
```

## Usage

Being made almost entirely of traits, Mini CQRS is very flexible but, of course,
it also requires a bit of boilerplate. For this reason, you can check out the
[examples directory](https://github.com/andreapavoni/mini_cqrs/tree/master/examples).


Here's a snippet extracted from the [game example](https://github.com/andreapavoni/mini_cqrs/tree/master/examples/game.rs):

```rust
let store = InMemoryEventStore::new();
let repo = Arc::new(Mutex::new(InMemoryRepository::new()));

let consumers = vec![GameEventConsumers::Counter(CounterConsumer::new(repo.clone()))];
let dispatcher: GameDispatcher = SimpleDispatcher::new(store, consumers);
let queries = AppQueries {};
let mut cqrs = Cqrs::new(dispatcher, queries);

let player_1 = Player { id: "player_1".to_string(), points: 0, };
let player_2 = Player { id: "player_2".to_string(), points: 0, };

let main_id = "0123456789abcdef".to_string();
let command = GameCommand::StartGame { player_1: player_1.clone(), player_2: player_2.clone(), goal: 3, },

let id = cqrs.execute( main_id.clone(), command.clone()).await?;

assert_eq!(id, main_id.clone());

let query = GetGameQuery::new(main_id.clone(), repo.clone());
let result = cqrs.queries().run(query.clone()).await?.unwrap();
```

Spoiler: the entire code for the above example is ~500 lines of code.

## Documentation

Code is decently documented and is being improved. For real working examples you can check the  
[examples](https://github.com/andreapavoni/mini_cqrs/tree/master/examples).

There's also a small work-in-progress PoC/experiment called [simple_counter](https://github.com/andreapavoni/simple_counter) which I'm using as a playground for a more complex real world use case in terms of code and libraries integration.

## Contributing

If you find any bugs or have any suggestions, please [open an issue](https://github.com/andreapavoni/mini_cqrs/issues).

## License

This project is open-source and available under the [MIT License](LICENSE).

## Status 

The project is somewhat usable for some real use cases, but I wouldn't recommend
it for production use.

## Roadmap

Here there's a roadmap for the ideas I've in mind:

- [ ] More `&`s, less `.clone()`s 
- [ ] Made a non async-Rust version
    - [ ] attempt to provide a multi-threaded example
    - [ ] define same traits as non async-Rust
    - [ ] use `features` flags to determine external dependencies
- [ ] Macros (only if they might reduce some boilerplate non-invasively) 
    - [ ] improve existing ones
    - [ ] use _derive-macros_
- [X] Docs
- [ ] Unit tests
- [ ] Publish pkg + docs.
