# Hotel: a complete CQRS application

This standalone package depends on `mini_cqrs_es` through a path dependency.
It uses the library's Toasty re-export for models and queries; no separate ORM
dependency or custom persistence adapter is required. Rust 1.95+ is required.

| File | Application responsibility |
| --- | --- |
| [domain.rs](domain.rs) | Hotel state, typed IDs, the `HotelEvent` family and commands |
| [read_models.rs](read_models.rs) | Hotel/room tables as Toasty models |
| [projection.rs](projection.rs) | Apply typed events using the supplied transaction executor |
| [queries.rs](queries.rs) | Query Toasty models and return an application DTO |
| [config.rs](config.rs) | Select separate event/read connection URLs from the environment |
| [main.rs](main.rs) | Wire the runner, optional snapshot cache, queries and rebuild CLI |

`HotelAggregate::Event` is the whole `HotelEvent` enum: initialization,
check-in and check-out are different variants. Commands emit these events;
aggregate recovery and projections apply them. Event history and snapshots
share one database. Hotel/room models and projection checkpoints share another.
Only event-derived read data can be rebuilt.

## Run locally with Turso

The library has no default driver. Explicitly enabling this application's
`turso` feature supplies local defaults `turso:hotel-events.db` and
`turso:hotel-read-models.db` when neither URL environment variable is set.
No Cloud service is involved. Run from the repository root:

```sh
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- init
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- create 1 3
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- check-in 1 2 Andrea
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- show 1
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- check-out 1 2
```

Run `init` **once**, against fresh databases. Ordinary commands connect without
DDL. Schema creation is not an idempotent migration or reset command. An
initialization failure may leave partially created tables; inspect/discard fresh
failed targets before retrying.

Override both URLs together to use different files (parent directories must
exist):

```sh
export EVENTS_DATABASE_URL=turso:/tmp/hotel-events.db
export READ_MODELS_DATABASE_URL=turso:/tmp/hotel-reads.db
```

## Run with PostgreSQL

Provision two empty databases, then configure the same application:

```sh
createdb hotel_events
createdb hotel_reads
export EVENTS_DATABASE_URL=postgresql://localhost/hotel_events
export READ_MODELS_DATABASE_URL=postgresql://localhost/hotel_reads
cargo run --manifest-path examples/hotel/Cargo.toml --features postgresql -- init
cargo run --manifest-path examples/hotel/Cargo.toml --features postgresql -- create 1 3
cargo run --manifest-path examples/hotel/Cargo.toml --features postgresql -- show 1
```

Supply credentials/TLS settings appropriate to your PostgreSQL installation in
the URLs. The driver feature is chosen at build time; the URL selects connections
at runtime. The library does not create PostgreSQL databases or read these ENV
variables itself. Keep the two databases physically separate; the example's
string equality check cannot detect different URLs referring to the same database.

## Recover projection delivery

A successful command prints its committed version. If an error reports that
events were already committed, fix the read-side failure and catch up without
repeating the command:

```sh
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- catch-up 1
```

The projector resumes its durable checkpoint, commits each model change with its
checkpoint, and skips duplicate versions. Keep one ordered writer per
projection/stream. An uncertain append outcome requires inspecting event history
before deciding whether to retry. There is no automatic command retry or
background projection worker.

## Rebuild into a replacement read database

Pause **all commands and live projection delivery**, then choose a fresh target:

```sh
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- rebuild turso:/tmp/hotel-rebuilt.db
```

For PostgreSQL, provision an empty replacement database first, then use
`--features postgresql -- rebuild postgresql://localhost/hotel_rebuilt`.
The command preflights supported aggregate types, creates the replacement read
schema and replays bounded pages through the same projection. Event history and
snapshots stay in the event database.

After success, set `READ_MODELS_DATABASE_URL` to the replacement and restart
before resuming writers. A failed target must not be published; discard it and
retry with a fresh database. The application does not coordinate running
processes or cut over automatically. Keep writers paused across preflight,
replay and cutover. Additional aggregate types require matching declarations and
projection consumers in the rebuild pipeline.

## Verify

The root `tests/hotel_app.rs` runs this application's actual domain, models,
projection and query code. Shared root suites cover storage, concurrency,
snapshot recovery, transactional projection failure and rebuild contracts on
both databases. CI also runs this standalone CLI against each backend.

```sh
cargo test --locked --features turso --test hotel_app
cargo fmt --manifest-path examples/hotel/Cargo.toml --check
cargo clippy --manifest-path examples/hotel/Cargo.toml --features turso -- -D warnings
```
