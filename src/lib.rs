use async_trait::async_trait;

// Error
#[derive(Debug)]
pub struct CqrsError(String);

impl CqrsError {
    pub fn new(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for CqrsError {
    fn from(msg: &str) -> Self {
        Self(msg.to_string())
    }
}

impl Display for CqrsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CqrsError {}

// Aggregate
#[async_trait]
pub trait Aggregate: Default + Sync + Send {
    type Command;
    type Event;

    async fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, CqrsError>;
    fn apply(&mut self, event: &Self::Event);
    fn aggregate_id(&self) -> &str;

    fn apply_events(&mut self, events: &Vec<Self::Event>) {
        for e in events.into_iter() {
            self.apply(&e);
        }
    }
}

// Event consumer
#[async_trait]
pub trait EventConsumer {
    type Event;

    async fn process<'a>(&self, event: &'a Self::Event);
}

// Event store
#[async_trait]
pub trait EventStore {
    type Event: Clone;

    async fn save_events(
        &mut self,
        aggregate_id: &str,
        events: &Vec<Self::Event>,
    ) -> Result<(), CqrsError>;
    async fn load_events(&self, aggregate_id: &str) -> Result<Vec<Self::Event>, CqrsError>;
}

use std::fmt::{Display, Formatter};

#[async_trait]
pub trait CommandDispatcher<A, C, E, ES, EC>
where
    A: Aggregate,
    C: Send + Sync,
    E: Send + Sync + Clone,
    ES: EventStore<Event = E>,
    EC: EventConsumer<Event = E>,
{
    async fn execute(&mut self, aggregate_id: &str, command: &C) -> Result<A, CqrsError>;
}

pub struct SimpleCommandDispatcher<A, C, E, ES, EC>
where
    A: Aggregate,
    C: Send + Sync,
    E: Send + Sync + Clone,
    ES: EventStore<Event = E>,
    EC: EventConsumer<Event = E>,
{
    event_store: ES,
    event_consumers: Vec<EC>,
    _marker_a: std::marker::PhantomData<A>,
    _marker_c: std::marker::PhantomData<C>,
}

impl<A, C, E, ES, EC> SimpleCommandDispatcher<A, C, E, ES, EC>
where
    A: Aggregate,
    C: Send + Sync,
    E: Send + Sync + Clone,
    ES: EventStore<Event = E>,
    EC: EventConsumer<Event = E>,
{
    pub fn new(event_store: ES, event_consumers: Vec<EC>) -> Self {
        Self {
            event_store,
            event_consumers,
            _marker_a: std::marker::PhantomData,
            _marker_c: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<A, C, E, ES, EC> CommandDispatcher<A, C, E, ES, EC>
    for SimpleCommandDispatcher<A, C, E, ES, EC>
where
    A: Aggregate + Aggregate<Event = E> + Aggregate<Command = C>,
    C: Send + Sync,
    E: Send + Sync + Clone,
    ES: EventStore<Event = E> + std::marker::Send + std::marker::Sync,

    EC: EventConsumer<Event = E> + std::marker::Send + std::marker::Sync,
{
    async fn execute(&mut self, aggregate_id: &str, command: &C) -> Result<A, CqrsError> {
        let mut aggregate = match self.event_store.load_events(aggregate_id).await {
            Ok(events) => {
                let mut aggregate = A::default();
                aggregate.apply_events(&events);
                aggregate
            }
            Err(_) => A::default(),
        };

        let events = aggregate.handle(command).await?;
        self.event_store.save_events(aggregate_id, &events).await?;

        aggregate.apply_events(&events);

        for consumer in &self.event_consumers {
            for event in &events {
                consumer.process(event).await;
            }
        }

        Ok(aggregate)
    }
}
