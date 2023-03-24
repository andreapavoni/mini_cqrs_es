use std::{
    fmt::{Debug, Display, Formatter},
    marker::PhantomData,
};

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
pub trait Aggregate: Debug + Default + Sync + Send {
    type Command;
    type Event;
    type Id: Clone + Send + Sync + Debug;

    async fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, CqrsError>;
    fn apply(&mut self, event: &Self::Event);
    fn aggregate_id(&self) -> Self::Id;
    fn set_aggregate_id(&mut self, id: Self::Id);

    fn apply_events(&mut self, events: &Vec<Self::Event>) {
        for e in events.into_iter() {
            self.apply(&e);
        }
    }
}

// Event consumer
#[async_trait]
pub trait EventConsumer: Sync + Send {
    type Event;

    async fn process<'a>(&mut self, event: &'a Self::Event);
}

// Event store
#[async_trait]
pub trait EventStore {
    type Event: Clone;
    type AggregateId: Clone;

    async fn save_events(
        &mut self,
        aggregate_id: Self::AggregateId,
        events: &Vec<Self::Event>,
    ) -> Result<(), CqrsError>;
    async fn load_events(
        &self,
        aggregate_id: Self::AggregateId,
    ) -> Result<Vec<Self::Event>, CqrsError>;
}

pub trait Repository: Send + Sync + Clone {}

#[async_trait]
pub trait Query: Send + Sync + Clone {
    type Output: Clone;
    type Repo: Repository;

    async fn run(&self, repo: Self::Repo) -> Result<Self::Output, CqrsError>;
}

#[async_trait]
pub trait ModelReader: Send {
    type Repo: Repository;
    type Query: Send + Sync + Clone + 'static;
    type Output: Send + Sync + 'static;

    async fn run(&self, query: Self::Query) -> Result<Self::Output, CqrsError>;
    async fn update(&mut self, data: Self::Output) -> Result<(), CqrsError>;
}

// Command dispatcher
#[async_trait]
pub trait Dispatcher<A, ES>: Send
where
    A: Aggregate,
    ES: EventStore<Event = A::Event>,
{
    async fn execute(&mut self, aggregate_id: A::Id, command: &A::Command) -> Result<A, CqrsError>;
}

pub struct SimpleDispatcher<A, ES>
where
    A: Aggregate,
    ES: EventStore<Event = A::Event>,
{
    event_store: ES,
    event_consumers: Vec<Box<dyn EventConsumer<Event = A::Event>>>,
    marker: PhantomData<A>,
}

impl<A, ES> SimpleDispatcher<A, ES>
where
    A: Aggregate,
    ES: EventStore<Event = A::Event>,
{
    pub fn new(
        event_store: ES,
        event_consumers: Vec<Box<dyn EventConsumer<Event = A::Event>>>,
    ) -> Self {
        Self {
            event_store,
            event_consumers,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<A, ES> Dispatcher<A, ES> for SimpleDispatcher<A, ES>
where
    A: Aggregate,
    ES: EventStore<Event = A::Event, AggregateId = A::Id> + Send + Sync,
    A::Command: Send + Sync,
    A::Event: Debug + Send + Sync,
{
    async fn execute(&mut self, aggregate_id: A::Id, command: &A::Command) -> Result<A, CqrsError> {
        let mut aggregate = match self.event_store.load_events(aggregate_id.clone()).await {
            Ok(events) => {
                let mut aggregate = A::default();
                aggregate.apply_events(&events);
                aggregate
            }
            Err(_) => A::default(),
        };

        aggregate.set_aggregate_id(aggregate_id.clone());

        let events = aggregate.handle(command).await?;
        self.event_store.save_events(aggregate_id, &events).await?;

        aggregate.apply_events(&events);

        for consumer in self.event_consumers.iter_mut() {
            for event in &events {
                consumer.process(event).await;
            }
        }

        Ok(aggregate)
    }
}

pub struct Cqrs<D, A, ES>
where
    D: Dispatcher<A, ES>,
    A: Aggregate,
    ES: EventStore<Event = A::Event, AggregateId = A::Id>,
{
    dispatcher: D,
    marker: PhantomData<(A, ES)>,
}

impl<D, A, ES> Cqrs<D, A, ES>
where
    D: Dispatcher<A, ES>,
    A: Aggregate,
    ES: EventStore<Event = A::Event, AggregateId = A::Id>,
{
    pub fn new(dispatcher: D) -> Self {
        Self {
            dispatcher,
            marker: PhantomData,
        }
    }

    pub async fn execute(
        &mut self,
        aggregate_id: A::Id,
        command: &A::Command,
    ) -> Result<(), CqrsError> {
        match self.dispatcher.execute(aggregate_id, command).await {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        }
    }

    pub async fn query<MR: ModelReader>(
        &self,
        reader: MR,
        query: MR::Query,
    ) -> Result<<MR as ModelReader>::Output, CqrsError> {
        reader.run(query).await
    }
}
