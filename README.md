# MiniCQRS/ES

A lightweight CQRS/event-sourcing library for Rust. Define your domain rules and
read models; the library handles event persistence, aggregate recovery,
transactional projection delivery and optional snapshots.

**Start here:** [the complete hotel application](examples/hotel/README.md) includes
commands, an event family, multiple Toasty read models, queries, snapshots,
catch-up and a read-database rebuild. It is a standalone package that depends on
this library exactly as an application would.

Version **0.12.0** requires Rust **1.95+**. Select a Toasty driver explicitly;
**no database driver is enabled by default**:

```toml
[dependencies]
mini_cqrs_es = { version = "0.12", features = ["postgresql"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["rt", "macros"] }
```

Use `features = ["turso"]` for local file-backed examples/testing. Toasty is
pinned to 0.11.0 and re-exported, so application models do not need a separate
Toasty dependency. Connection URLs and database provisioning belong to the
application; the library has no filesystem or environment-variable policy.

## What you build, what the library handles

You write domain state, events, commands, projections and queries. **Toasty is
re-exported as `mini_cqrs_es::toasty` for your read models and native queries.**
You do not select a separate Toasty implementation of CQRS: the built-in types
are named for their responsibilities.

| Type | Responsibility |
| --- | --- |
| `EventLog` | Persist immutable events and enforce expected stream versions |
| `ReadModels` | Open your read database and register framework checkpoint storage |
| `Projection` | Your logic for updating read models from a typed event |
| `Projector` | Run that logic and commit its checkpoint in the same transaction |
| `SimpleCqrs` | Recover state, handle a command, append events, then deliver them |
| `SnapshotCache` | Optional stored state that speeds up aggregate recovery |

```mermaid
flowchart LR
    Command --> State[Recovered aggregate state]
    State --> Events[Domain events]
    Events --> EventLog
    EventLog --> Projector
    Projector --> ReadModels
    ReadModels --> Query
    EventLog -->|replay| State
```

Keep **two databases**: event history and its snapshot cache in one; your
read models and their checkpoints in the other. Read models can be rebuilt from
events. Data that was never recorded in events needs another recovery source.

## Build a counter

The steps below show a small counter. For a complete runnable application,
follow [the hotel walkthrough](examples/hotel/README.md).

### 1. Define state and the facts that change it

```rust
use mini_cqrs_es::*;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Counter { value: u32 }

#[derive(Serialize, Deserialize)]
enum CounterEvent {
    Added { amount: u32 },
    Reset,
}

impl EventPayload for CounterEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Added { .. } => "counter.added",
            Self::Reset => "counter.reset",
        }
    }
}

impl Aggregate for Counter {
    type Id = String;
    type Event = CounterEvent;
    fn aggregate_type() -> &'static str { "counter" }
    async fn apply(&mut self, event: &CounterEvent) {
        match event {
            CounterEvent::Added { amount } => self.value += amount,
            CounterEvent::Reset => self.value = 0,
        }
    }
}
```

**`Aggregate::Event` is the aggregate's entire event family, usually an enum—not
one individual kind of event.** `CounterEvent` groups the different facts that can
change a counter. Each variant has its own persisted event kind. The aggregate
and its projections match on the variants they receive; adding a variant requires
updating exhaustive matches. A command returns `Vec<CounterEvent>` and may emit
zero, one or several events, including different variants.

Aggregate state contains domain data. The runtime owns identity and stream
version; commands do not maintain those fields. State needs Serde only when
using snapshots. Events always use Serde. Neither state nor events require
`Clone`; event kinds do not depend on display text.

Choose stable aggregate and event names: they identify persisted history.

### 2. Define a command that enforces your rules

```rust
struct Add(u32);

impl Command for Add {
    type Aggregate = Counter;
    async fn handle(&self, state: &Counter) -> Result<Vec<CounterEvent>, CqrsError> {
        if state.value.checked_add(self.0).is_none() {
            return Err(CqrsError::domain("counter would overflow"));
        }
        Ok(vec![CounterEvent::Added { amount: self.0 }])
    }
}

struct Reset;
impl Command for Reset {
    type Aggregate = Counter;
    async fn handle(&self, _: &Counter) -> Result<Vec<CounterEvent>, CqrsError> {
        Ok(vec![CounterEvent::Reset])
    }
}
```

Commands inspect recovered state and return facts to persist. For business rule
failures return `CqrsError::domain(...)`; use `invariant(...)` for request/routing
preconditions. Avoid irreversible effects in handlers: concurrent appends can
still reject the command's events.

### 3. Model the read side with Toasty

```rust
#[derive(Debug, toasty::Model)]
struct CounterView {
    #[key]
    id: String,
    value: u32,
}
```

Read models can differ completely from aggregate state. A projection may update
multiple tables. Queries can return models or application DTOs.

### 4. Define the projection

```rust
struct CounterProjection;

impl Projection for CounterProjection {
    type Aggregate = Counter;
    fn name() -> &'static str { "counter-total" }

    async fn apply(
        &self,
        event: &StoredEvent,
        payload: CounterEvent,
        executor: &mut dyn toasty::Executor,
    ) -> Result<(), CqrsError> {
        let previous = CounterView::filter_by_id(&event.aggregate_id)
            .first().exec(&mut *executor).await
            .map_err(|e| CqrsError::storage("load counter view", e))?
            .map_or(0, |row| row.value);
        let value = match payload {
            CounterEvent::Added { amount } => previous.checked_add(amount)
                .ok_or_else(|| CqrsError::domain("counter read model overflow"))?,
            CounterEvent::Reset => 0,
        };
        CounterView::upsert_by_id(&event.aggregate_id)
            .value(value).exec(executor).await
            .map_err(|e| CqrsError::storage("update counter view", e))?;
        Ok(())
    }
}
```

Use the supplied executor for every mutation. The `Projector` owns transaction
and checkpoint handling: it filters unrelated aggregate types, skips duplicates,
rejects gaps, decodes the event and commits model updates with its checkpoint.
Do not commit/roll back inside a projection or perform external notifications.
Keep one ordered writer per projection/stream. A projector is a runner, not a
background worker.

### 5. Open the databases and wire the runner

```rust
let events = EventLog::connect(&std::env::var("EVENTS_DATABASE_URL")?).await?;
let reads = ReadModels::builder()
    .register::<CounterView>()
    .connect(&std::env::var("READ_MODELS_DATABASE_URL")?).await?;

// First-time setup only, against fresh provisioned databases:
events.create_schema().await?;
reads.create_schema().await?;
let projector = reads.projector(CounterProjection);
let cqrs = SimpleCqrs::new(events.clone())
    .with_consumer(projector.clone());

let id = "counter-1".to_owned();
let receipt = cqrs.execute(&id, &Add(5), EventMetadata::default()).await?;
// Another command, another variant of CounterEvent:
let reset_receipt = cqrs.execute(&id, &Reset, EventMetadata::default()).await?;
```

Register only your application read models. Event, snapshot and checkpoint
storage are managed by the library. `connect` performs no DDL; use it during
ordinary startup. `create_schema` explicitly creates the registered tables and
indexes in a **fresh database**. It is not an idempotent initializer, schema
migration tool, or reset operation. Provision PostgreSQL databases yourself;
for Turso, use fresh file URLs with existing parent directories. Do not call
`create_schema` every time the application starts.

The runner shares one event store between recovery and append. Add consumers
with `with_consumer` or `with_consumers`; registration order is preserved.
Pass metadata when you need command/correlation/actor context. Metadata IDs do
not implement deduplication, tenant isolation or authorization.

Snapshots are an optional one-line change, before executing commands:

```rust
let cqrs = cqrs.with_snapshots(events.snapshot_cache());
```

They use the event database's existing pool. Recovery loads the last saved state
and replays its newer events. Missing snapshots trigger full replay; corrupt
snapshots or storage errors return errors. Snapshots are disposable caches, not
event history. No separate snapshot model or connection is needed.

### 6. Query the read model

Query models directly through `reads.database()`, or define a reusable query:

```rust
struct GetCounter { database: toasty::Db, id: String }

impl Query for GetCounter {
    type Output = Result<Option<u32>, CqrsError>;
    async fn apply(&self) -> Self::Output {
        CounterView::filter_by_id(&self.id).first()
            .exec(&mut self.database.clone()).await
            .map(|row| row.map(|row| row.value))
            .map_err(|e| CqrsError::storage("query counter", e))
    }
}

let value = cqrs.query(&GetCounter { database: reads.database(), id: id.clone() }).await?;
```

Queries own their data access and error type. There is no generic repository
layer to implement.

## Failures and recovery

Successful execution returns a receipt with the aggregate ID, committed stream
version and event IDs. Event persistence and downstream processing are separate:

| `ExecutionError` | Meaning / action |
| --- | --- |
| `NotCommitted` | Failure before append, or a version conflict rejecting this append |
| `CommitUnknown` | Append outcome is uncertain; reconcile history before retrying |
| `Committed` | Events were appended, but application/projection/snapshot processing failed; inspect the receipt and stage |

A committed failure does **not** mean the command should run again. Once its
cause is resolved, repair projection delivery with:

```rust
projector.catch_up(&events, &id).await?;
```

It resumes that projection's durable checkpoint through one captured event tip.
Earlier successful deliveries remain applied after a later failure. Operational
errors preserve their original source in `CqrsError::Storage`. Cancellation gives
no command outcome, and a commit error can be uncertain. The library does not
retry commands or promise distributed/exactly-once delivery.

### Rebuild a read database

Pause writers and live projection delivery, then preflight the full bounded
stream catalog before creating a replacement:

```rust
let prepared = ReplayPlan::new()
    .with_aggregate::<Counter>()
    .prepare(&events).await?;
let replacement = ReadModels::builder()
    .register::<CounterView>()
    .connect(&std::env::var("REBUILD_DATABASE_URL")?).await?;
replacement.create_schema().await?;
let projections = EventConsumers::new()
    .with(replacement.projector(CounterProjection));
let progress = prepared.run(&projections).await?;
// Publish the replacement only now; keep writers paused until cutover finishes.
```

Provision a fresh, separate destination before this operation. Schema creation
and replay can leave a partial target on failure; discard that target and retry
with another fresh database. The library neither checks URL equivalence nor
publishes/replaces databases. Applications must prevent accidental use of active
targets and coordinate cutover.
Declare every supported aggregate type and supply matching projection consumers;
the declarations do not inspect or verify the pipeline's coverage. Multiple
consumers are not one atomic transaction. `ReplayError` retains the failing
stream when known, confirmed delivery progress and the original source.

Only event-derived data can be reconstructed. Keep external notifications out
of replay. Change a projection's logic by rebuilding its output; changing its
checkpoint name alone does not clear populated model tables.

## Advanced integration

- **Schema ownership:** keep event/support tables separate from read models and
  checkpoints. `create_schema` is explicit first-time setup; applications own
  subsequent schema changes and provisioning.
- **Native Toasty configuration:** use `toasty::Db::builder()` with a model set
  and `EventLog::new(db)` for custom pools. `EventLog::models()` registers all
  framework persistence records for that builder. Event/snapshot models live under
  `persistence`; `ReadModels` registers checkpoints automatically on the default
  builder. Raw event updates/deletes bypass the log's invariants.
- **Custom stores/managers:** implement `EventStore`/`SnapshotStore` and use
  `Arc<YourStore>` to share a non-`Clone` store. `SimpleCqrs::from_parts` supports
  custom aggregate managers; callers must keep recovery and append on the same
  event history. Most applications do not need these interfaces.
- **Event evolution:** `SCHEMA_VERSION` defaults to one and applies to the whole
  event family. Default decoding validates revision and kind. Override
  `decode(kind, revision, payload)` for explicit historical upgrades, then call
  `decode_current` to validate the current variant. History is never rewritten.
  See [tested upgrade examples](tests/api_workflows.rs). Snapshot state formats
  evolve independently and may require cache invalidation.
- **Backend support:** the shared storage, snapshot and projection contracts run
  against file-backed Turso and PostgreSQL in CI. Backend independence does not
  remove differences in types, constraints, collation or errors. In Toasty 0.11,
  a competing PostgreSQL append can surface a unique-constraint failure as a
  storage error rather than a version conflict; execution conservatively reports
  an uncertain append outcome. Reconcile history before retrying. No global event
  order or online rebuild/cutover protocol is provided.

## Examples and verification

Run [the standalone hotel app](examples/hotel/README.md) with an explicit driver:

```sh
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- init
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- create 1 3
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- check-in 1 2 Andrea
cargo run --manifest-path examples/hotel/Cargo.toml --features turso -- show 1
```

The example alone supplies local file URLs when `turso` is enabled. For PostgreSQL,
select `--features postgresql` and set both `EVENTS_DATABASE_URL` and
`READ_MODELS_DATABASE_URL` to separate, provisioned PostgreSQL databases.

```sh
cargo test --locked --features turso --all-targets
cargo fmt --check
cargo fmt --manifest-path examples/hotel/Cargo.toml --check
cargo clippy --locked --features turso --all-targets -- -D warnings
```

To run the same contracts on PostgreSQL, use **disposable test databases** and
credentials allowed to create schemas, then run:

```sh
EVENTS_DATABASE_URL=postgresql://postgres:postgres@localhost/events \
READ_MODELS_DATABASE_URL=postgresql://postgres:postgres@localhost/read_models \
cargo test --locked --features postgresql --all-targets
```

PostgreSQL tests create uniquely named schemas per fixture. They do not clean
those schemas up; discard the test databases afterward. Both URLs must be set
together. CI provisions disposable databases, runs both backend suites, and runs
the complete application including replay into a fresh database. The core is
also tested and compiled with no driver enabled. `Cargo.lock` is ignored for
this library; omit `--locked` on the first local resolution.

The project is MIT-licensed.
