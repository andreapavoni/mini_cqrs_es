use std::num::NonZeroUsize;

use crate::{CqrsError, EventConsumers, EventStore, StoredEvent};

/// Default record-count limit for event pages and catalog collectors.
pub const DEFAULT_PAGE_SIZE: NonZeroUsize = NonZeroUsize::new(256).unwrap();

/// A bounded per-stream reader with an endpoint captured once at opening.
///
/// Each page is fully validated before it is returned. Appends beyond the
/// captured tip are excluded. Immutable history is required: this is not a
/// database transaction spanning pages. The position tracks validated reads,
/// not successful consumer delivery, and must not be treated as a checkpoint.
pub struct EventStreamReader<'a, ES: EventStore + ?Sized> {
    store: &'a ES,
    aggregate_type: &'a str,
    aggregate_id: &'a str,
    position: u64,
    through_version: u64,
    page_size: NonZeroUsize,
}

impl<'a, ES: EventStore + ?Sized> EventStreamReader<'a, ES> {
    /// Captures the current tip. `after_version` is the last version to skip;
    /// a cursor beyond the tip is an error, equality is a completed read.
    pub async fn open(
        store: &'a ES,
        aggregate_type: &'a str,
        aggregate_id: &'a str,
        after_version: u64,
        page_size: NonZeroUsize,
    ) -> Result<Self, CqrsError> {
        let through_version = store.stream_version(aggregate_type, aggregate_id).await?;
        if after_version > through_version {
            return Err(CqrsError::EventStore(
                "replay cursor exceeds stream tip".into(),
            ));
        }
        Ok(Self {
            store,
            aggregate_type,
            aggregate_id,
            position: after_version,
            through_version,
            page_size,
        })
    }

    /// The endpoint captured at opening, unchanged by subsequent appends.
    pub fn through_version(&self) -> u64 {
        self.through_version
    }
    /// Last validated read version. Does not confirm projection application.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Reads one bounded page, rejecting short, empty, oversized, out-of-order,
    /// wrong-stream, or out-of-range results without advancing the position.
    pub async fn next_page(&mut self) -> Result<Option<Vec<StoredEvent>>, CqrsError> {
        if self.position == self.through_version {
            return Ok(None);
        }
        let events = self
            .store
            .load_events_after(
                self.aggregate_type,
                self.aggregate_id,
                self.position,
                self.through_version,
                self.page_size,
            )
            .await?;
        let expected = (self.through_version - self.position).min(self.page_size.get() as u64);
        if events.len() as u64 != expected {
            return Err(CqrsError::EventStore(format!(
                "invalid event page: expected {expected} events, got {}",
                events.len(),
            )));
        }
        let mut version = self.position;
        for event in &events {
            version = version
                .checked_add(1)
                .ok_or_else(|| CqrsError::EventStore("stream version overflow".into()))?;
            if event.schema_version == 0
                || event.event_type.is_empty()
                || event.aggregate_type != self.aggregate_type
                || event.aggregate_id != self.aggregate_id
                || event.version != version
                || version > self.through_version
            {
                return Err(CqrsError::EventStore(
                    "invalid event page identity or version".into(),
                ));
            }
        }
        self.position = version;
        Ok(Some(events))
    }
}

/// Replays a full stream in bounded pages into projection-only consumers.
/// Returns the tip captured at opening; later appends are excluded.
///
/// Each page's envelopes are validated before delivery. Earlier events/pages
/// and consumers can remain applied after failure/cancellation. Use a fresh target or consumers
/// with transactional checkpoints. No writes, commands, notifications, snapshot
/// recovery, cross-stream ordering, online catch-up, or publication are performed.
pub async fn replay_stream(
    store: &impl EventStore,
    aggregate_type: &str,
    aggregate_id: &str,
    projections: &EventConsumers,
) -> Result<u64, CqrsError> {
    replay_from(
        store,
        aggregate_type,
        aggregate_id,
        0,
        DEFAULT_PAGE_SIZE,
        projections,
    )
    .await
}

/// Replays strictly after the last successfully applied version, to a fixed tip.
/// The caller must ensure the target actually represents `after_version` and
/// coordinate live delivery/cutover. Failure does not return a delivery checkpoint.
pub async fn replay_from(
    store: &impl EventStore,
    aggregate_type: &str,
    aggregate_id: &str,
    after_version: u64,
    page_size: NonZeroUsize,
    projections: &EventConsumers,
) -> Result<u64, CqrsError> {
    let mut reader = EventStreamReader::open(
        store,
        aggregate_type,
        aggregate_id,
        after_version,
        page_size,
    )
    .await?;
    while let Some(events) = reader.next_page().await? {
        for event in &events {
            projections.process(event).await?;
        }
    }
    Ok(reader.through_version())
}

/// Confirmed complete stream/event deliveries. An error may leave additional
/// effects from consumers earlier in the failing event's pipeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplayProgress {
    pub streams: u64,
    pub events: u64,
}

/// Recovery failure with the stream (if known), confirmed progress, and source.
#[derive(Debug, thiserror::Error)]
#[error("replay failed for {stream:?} after {progress:?}: {source}")]
pub struct ReplayError {
    pub stream: Option<crate::StreamKey>,
    pub progress: ReplayProgress,
    #[source]
    pub source: CqrsError,
}

/// Offline rebuild preflight. Declare every aggregate type supported by the
/// projection-only pipeline; consumers must filter unrelated stream types.
/// Writers must remain paused across preparation, replay, and cutover.
pub struct ReplayPlan {
    types: std::collections::HashSet<&'static str>,
    page_size: NonZeroUsize,
}

impl ReplayPlan {
    pub fn new() -> Self {
        Self {
            types: Default::default(),
            page_size: DEFAULT_PAGE_SIZE,
        }
    }

    pub fn with_aggregate<A: crate::Aggregate>(mut self) -> Self {
        self.types.insert(A::aggregate_type());
        self
    }

    pub fn with_page_size(mut self, size: NonZeroUsize) -> Self {
        self.page_size = size;
        self
    }

    /// Preflights the entire bounded catalog before target creation or callbacks.
    /// Retains only registered type names and one catalog cursor, not the catalog.
    pub async fn prepare<ES: EventStore + ?Sized>(
        self,
        events: &ES,
    ) -> Result<PreparedReplay<'_, ES>, ReplayError> {
        let mut after = None;
        loop {
            let page = self.page(events, after.as_ref()).await?;
            if page.is_empty() {
                break;
            }
            after = page.last().cloned();
        }
        Ok(PreparedReplay { plan: self, events })
    }

    async fn page<ES: EventStore + ?Sized>(
        &self,
        events: &ES,
        after: Option<&crate::StreamKey>,
    ) -> Result<Vec<crate::StreamKey>, ReplayError> {
        let page = events
            .list_streams_after(after, self.page_size)
            .await
            .map_err(|source| ReplayError {
                stream: None,
                progress: ReplayProgress::default(),
                source,
            })?;
        let mut seen = std::collections::HashSet::new();
        if page.len() > self.page_size.get()
            || page
                .iter()
                .any(|key| Some(key) == after || !seen.insert(key))
        {
            return Err(ReplayError {
                stream: None,
                progress: ReplayProgress::default(),
                source: CqrsError::EventStore("invalid stream discovery page".into()),
            });
        }
        for key in &page {
            if !self.types.contains(key.0.as_str()) {
                return Err(ReplayError {
                    stream: Some(key.clone()),
                    progress: ReplayProgress::default(),
                    source: CqrsError::EventFormat(format!(
                        "no projection declared for stream type {}",
                        key.0
                    )),
                });
            }
        }
        Ok(page)
    }
}

/// Prepared offline rebuild. Preparation is not a snapshot or writer lock;
/// applications keep writers paused and create a fresh unpublished destination.
/// Only event-derived output can be reconstructed. A failed target stays unpublished.
pub struct PreparedReplay<'a, ES: EventStore + ?Sized> {
    plan: ReplayPlan,
    events: &'a ES,
}

impl<ES: EventStore + ?Sized> PreparedReplay<'_, ES> {
    /// Replays all supported streams in bounded catalog/event pages. No commands,
    /// snapshots, file operations, schema changes, or external notifications.
    /// Multiple consumers are not one atomic transaction; use their checkpoints
    /// to retry, or discard the failed target and prepare a fresh rebuild.
    pub async fn run(self, projections: &EventConsumers) -> Result<ReplayProgress, ReplayError> {
        let mut progress = ReplayProgress::default();
        let mut after = None;
        loop {
            let page = self
                .plan
                .page(self.events, after.as_ref())
                .await
                .map_err(|mut error| {
                    error.progress = progress;
                    error
                })?;
            if page.is_empty() {
                return Ok(progress);
            }
            for key in &page {
                let result = async {
                    let mut reader = EventStreamReader::open(
                        self.events,
                        &key.0,
                        &key.1,
                        0,
                        self.plan.page_size,
                    )
                    .await?;
                    while let Some(events) = reader.next_page().await? {
                        for event in &events {
                            projections.process(event).await?;
                            progress.events = progress
                                .events
                                .checked_add(1)
                                .ok_or_else(|| CqrsError::invariant("replay progress overflow"))?;
                        }
                    }
                    progress.streams = progress
                        .streams
                        .checked_add(1)
                        .ok_or_else(|| CqrsError::invariant("replay progress overflow"))?;
                    Ok(())
                }
                .await;
                result.map_err(|source| ReplayError {
                    stream: Some(key.clone()),
                    progress,
                    source,
                })?;
            }
            after = page.last().cloned();
        }
    }
}

impl Default for ReplayPlan {
    fn default() -> Self {
        Self::new()
    }
}
