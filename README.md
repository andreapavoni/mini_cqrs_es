# Mini CQRS

Simple, minimal, opinionated micro-framework to implement CQRS/ES in Rust. There are already
a lot of opinionated libraries to do that but, in fact, I didn't agree with their 
opinions and I've seen an opportunity to improve my knowledge and practice with Rust.

The main goals of this project are:

- Keep things simple and easy to understand and use
- Provide Rust traits as a guideline to implement the necessary building blocks
- Leave as much implementation choices as possible to the developer
- Provide some minimal glue code to assemble the pieces


There's a basic [example](examples/simple.rs) to show how it works and it's acting as a test too. The
library source still fits into a single file, maybe it will be split later in smaller files.


## Status 

Despite it's very simple working code, the project is not yet usable for real use cases,
it might be considered as a starting point for something with more capabilites.

## Roadmap

Here's the plan in (sort of) descending order of importance:

- [x] Aggregate trait
  - [x] execute commands
  - [x] apply events
  - [ ] serialization (for snapshotting)
- [x] EventStore trait
  - [x] event serialization
  - [ ] snapshotting
  - [ ] event versioning and upcasting
- [x] EventConsumer trait
- [x] ReadModel trait
- [x] Command dispatcher trait
  - [x] SimpleDispatcher implementation (default)
  - [x] execute commands
- [x] Cqrs wrapper
  - [x] execute commands
  - [x] run queries
- [x] macros (only if they might reduce some boilerplate non-invasively) 
- [ ] more docs
- [ ] add more examples
- [ ] unit tests
- [ ] publish the pkg on crates.io 


## Contributing

If you find any bugs or have any suggestions, please [open an issue](https://github.com/andreapavoni/mini_cqrs/issues).

## Copyright

Released under the MIT License by Andrea Pavoni.
