use std::{future::Future, sync::Arc};

use crate::{
    Aggregate, AggregateManager, Command, CqrsError, EventConsumer, EventConsumers, EventMetadata,
    EventStore, NewEvent, SimpleAggregateManager, SnapshotAggregateManager, SnapshotStore,
    query::QueryRunner,
};

/// The result of a confirmed successful event append for one command execution.
///
/// This is not a command-deduplication record or a guarantee that projections
/// finished. A successful empty append has no event IDs and retains the loaded
/// stream version. Versions and event IDs come from the event store's response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionReceipt<Id> {
    pub aggregate_id: Id,
    pub version: u64,
    /// IDs of this invocation's events, in append order.
    pub event_ids: Vec<String>,
}

/// Processing boundaries reached after a confirmed successful event append.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PostCommitStage {
    AggregateApply,
    Consumers,
    AggregateStore,
}

/// A command failure classified by what is known about its event append.
///
/// Classification relies on the event store's contract: success confirms the
/// append, and `Conflict` rejects it. Other append errors are conservatively
/// uncertain. This describes this invocation's event append, not side effects
/// inside application commands/managers or concurrent commands.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExecutionError<Id> {
    /// Failure before append, or an expected-version conflict rejecting append.
    #[error("command events were not committed: {source}")]
    NotCommitted {
        #[source]
        source: CqrsError,
    },
    /// Append returned an error without establishing whether it committed.
    /// Reconcile with stored history before deciding whether to retry.
    #[error("event append outcome is unknown: {source}")]
    CommitUnknown {
        #[source]
        source: CqrsError,
    },
    /// Append succeeded, but subsequent processing failed. Earlier consumers or
    /// events may already have been processed; do not blindly repeat the command.
    #[error("event append succeeded, but {stage:?} failed: {source}")]
    Committed {
        receipt: ExecutionReceipt<Id>,
        stage: PostCommitStage,
        #[source]
        source: CqrsError,
    },
}

/// The `Cqrs` trait represents the main entry point of a CQRS application.
///
/// It orchestrates command execution. Implementations determine how events
/// are dispatched after being saved (e.g., synchronous consumers, broadcast channels).
///
/// All methods take `&self`, so implementations can be wrapped in `Arc` for concurrent use.
/// The aggregate parameter lets managers constrain the state types they support.
///
/// ```rust
/// use mini_cqrs_es::{
///     Aggregate, Command, Cqrs, EventMetadata, ExecutionError, ExecutionReceipt,
/// };
///
/// async fn run<A, C, E>(
///     engine: &E,
///     id: &A::Id,
///     command: &C,
///     metadata: EventMetadata,
/// ) -> Result<ExecutionReceipt<A::Id>, ExecutionError<A::Id>>
/// where
///     A: Aggregate,
///     C: Command<Aggregate = A>,
///     E: Cqrs<A>,
/// {
///     engine.execute(id, command, metadata).await
/// }
/// ```
pub trait Cqrs<A: Aggregate>: QueryRunner + Send + Sync {
    /// Executes a command with caller-supplied metadata on every emitted event.
    /// Returns a receipt after all processing succeeds. A post-append failure
    /// retains the receipt and failing stage in [`ExecutionError::Committed`].
    ///
    /// Pass [`EventMetadata::default()`] when no context is needed. No metadata
    /// IDs are generated, and command IDs do not provide deduplication. Tenant
    /// metadata is not authorization or stream isolation. There are no automatic
    /// command retries. Cancellation returns no outcome: dropping the future does
    /// not establish that events were not committed.
    fn execute<C>(
        &self,
        aggregate_id: &A::Id,
        command: &C,
        metadata: EventMetadata,
    ) -> impl Future<Output = Result<ExecutionReceipt<A::Id>, ExecutionError<A::Id>>> + Send
    where
        C: Command<Aggregate = A>;
}

/// A synchronous-consumer implementation of the [`Cqrs`] trait.
///
/// Events are dispatched through [`EventConsumers`], which processes each consumer
/// sequentially after events are saved.
///
/// The execution flow:
/// 1. Load aggregate from the aggregate manager
/// 2. Execute the command, getting domain events or a semantic error
///    (`Domain` for business rules, `CommandInvariant` for application preconditions)
/// 3. Wrap domain events into `NewEvent` structs
/// 4. Save events to the event store (with optimistic concurrency check)
/// 5. Apply events to the aggregate
/// 6. Process events through consumers
/// 7. Store the aggregate (e.g., snapshot)
/// 8. Return the execution receipt
pub struct SimpleCqrs<ES, AM>
where
    AM: Send + Sync,
    ES: EventStore,
{
    aggregate_manager: AM,
    event_store: ES,
    consumers: EventConsumers,
}

impl<ES, AM> SimpleCqrs<ES, AM>
where
    AM: Send + Sync,
    ES: EventStore,
{
    /// Advanced constructor for custom managers. The caller must ensure recovery
    /// and append use the same history; prefer `new` for the default setup.
    pub fn from_parts(aggregate_manager: AM, event_store: ES, consumers: EventConsumers) -> Self {
        Self {
            aggregate_manager,
            event_store,
            consumers,
        }
    }
}

impl<ES: EventStore> SimpleCqrs<Arc<ES>, SimpleAggregateManager<Arc<ES>>> {
    /// Shares one event store between recovery and append, without requiring Clone.
    pub fn new(event_store: ES) -> Self {
        let events = Arc::new(event_store);
        Self::from_parts(
            SimpleAggregateManager::new(events.clone()),
            events,
            EventConsumers::new(),
        )
    }

    /// Opt into snapshots while retaining the same event history for recovery.
    pub fn with_snapshots<SS: SnapshotStore>(
        self,
        snapshots: SS,
    ) -> SimpleCqrs<Arc<ES>, SnapshotAggregateManager<Arc<ES>, SS>> {
        SimpleCqrs::from_parts(
            SnapshotAggregateManager::new(self.event_store.clone(), snapshots),
            self.event_store,
            self.consumers,
        )
    }
}

impl<ES: EventStore, AM: Send + Sync> SimpleCqrs<ES, AM> {
    pub fn with_consumer(mut self, consumer: impl EventConsumer + 'static) -> Self {
        self.consumers = self.consumers.with(consumer);
        self
    }

    pub fn with_consumers(mut self, consumers: EventConsumers) -> Self {
        self.consumers = self.consumers.extend(consumers);
        self
    }
}

impl<A, ES, AM> Cqrs<A> for SimpleCqrs<ES, AM>
where
    A: Aggregate,
    AM: AggregateManager<A>,
    ES: EventStore,
{
    async fn execute<C>(
        &self,
        aggregate_id: &A::Id,
        command: &C,
        metadata: EventMetadata,
    ) -> Result<ExecutionReceipt<A::Id>, ExecutionError<A::Id>>
    where
        C: Command<Aggregate = A>,
    {
        let mut aggregate = self
            .aggregate_manager
            .load(aggregate_id)
            .await
            .map_err(|source| ExecutionError::NotCommitted { source })?;

        if aggregate.id() != aggregate_id {
            return Err(ExecutionError::NotCommitted {
                source: CqrsError::invariant("loaded aggregate belongs to a different stream"),
            });
        }
        let domain_events = command
            .handle(aggregate.state())
            .await
            .map_err(|source| ExecutionError::NotCommitted { source })?;

        let current_version = aggregate.version();
        let new_events: Vec<NewEvent> = domain_events
            .into_iter()
            .map(|payload| NewEvent::from_payload(payload, metadata.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| ExecutionError::NotCommitted { source })?;

        let events = self
            .event_store
            .save_events(
                C::Aggregate::aggregate_type(),
                &aggregate_id.to_string(),
                &new_events,
                current_version,
            )
            .await
            .map_err(|source| match source {
                CqrsError::Conflict { .. } => ExecutionError::NotCommitted { source },
                _ => ExecutionError::CommitUnknown { source },
            })?;

        let new_version = events.last().map(|e| e.version).unwrap_or(current_version);
        let receipt = ExecutionReceipt {
            aggregate_id: aggregate_id.clone(),
            version: new_version,
            event_ids: events.iter().map(|event| event.id.clone()).collect(),
        };
        if let Err(source) = aggregate.apply_events(&events).await {
            return Err(ExecutionError::Committed {
                receipt,
                stage: PostCommitStage::AggregateApply,
                source,
            });
        }
        for event in events.iter() {
            if let Err(source) = self.consumers.process(event).await {
                return Err(ExecutionError::Committed {
                    receipt,
                    stage: PostCommitStage::Consumers,
                    source,
                });
            }
        }

        if let Err(source) = self.aggregate_manager.store(&aggregate).await {
            return Err(ExecutionError::Committed {
                receipt,
                stage: PostCommitStage::AggregateStore,
                source,
            });
        }

        Ok(receipt)
    }
}

/// Implements `QueryRunner` so you can call `cqrs.query(&q).await`.
impl<ES, AM> QueryRunner for SimpleCqrs<ES, AM>
where
    AM: Send + Sync,
    ES: EventStore,
{
}
