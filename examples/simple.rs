/// A basic example to show how to use the `mini_cqrs` framework.
/// Here we have a Counter aggregate which accepts increment/decrement commands and emits
/// incremented/decremented events. Two consumer will just print the events they receive.
use std::collections::HashMap;

use async_trait::async_trait;

use mini_cqrs::{Aggregate, CqrsError, Dispatcher, EventConsumer, EventStore, SimpleDispatcher, Event, act_as_event, EventPayload};
use serde::{Serialize, Deserialize};

// Aggregate
#[derive(Default, Clone, Debug)]
struct CounterState {
    id: String,
    count: u32,
}

#[async_trait]
impl Aggregate for CounterState {
    type Command = CounterCommand;
    type Event = CounterEvent;
    type Id = String;

    async fn handle(&self, command: Self::Command) -> Result<Vec<Event>, CqrsError> {
        match command {
            CounterCommand::Increment(amount) => Ok(vec![CounterEvent::Incremented(amount).into()]),
            CounterCommand::Decrement(amount) => {
                if self.count < amount {
                    return Err(CqrsError::new(format!(
                        "COMMANDERROR: Decrement amount {} is greater than current count {}",
                        amount, self.count
                    )));
                }
                Ok(vec![CounterEvent::Decremented(amount).into()])
            }
        }
    }

    fn apply(&mut self, event: &Self::Event) {
        match event {
            CounterEvent::Incremented(amount) => self.count += amount,
            CounterEvent::Decremented(amount) => self.count -= amount,
        };
    }

    fn aggregate_id(&self) -> Self::Id {
        self.id.clone()
    }

    fn set_aggregate_id(&mut self, id: Self::Id) {
        self.id = id.clone();
    }
}

// Commands
#[derive(Debug, PartialEq, Clone)]
enum CounterCommand {
    Increment(u32),
    Decrement(u32),
}

// Events
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
enum CounterEvent {
    Incremented(u32),
    Decremented(u32),
}

impl EventPayload for CounterEvent {
 fn aggregate_id(&self) -> String {
         "counter".to_string()
 }
}

impl ToString for CounterEvent {
    fn to_string(&self) -> String {
        match self {
            CounterEvent::Incremented(amount) => format!("Incremented {}", amount),
            CounterEvent::Decremented(amount) => format!("Decremented {}", amount),
        }
    }
}

act_as_event!(CounterEvent);

// Consumer
struct PrintEventConsumer {}

#[async_trait]
impl EventConsumer for PrintEventConsumer {
    async fn process<'a>(&mut self, event: &'a Event) {
        println!("C: Consuming event: {:#?}", event);
    }
}

struct AnotherEventConsumer {}

#[async_trait]
impl EventConsumer for AnotherEventConsumer {
    async fn process<'a>(&mut self, event: &'a Event) {
        println!("C: Consuming event on another consumer: {:#?}", event);
    }
}

// Event Store

struct InMemoryEventStore {
    events: HashMap<String, Vec<Event>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        InMemoryEventStore {
            events: HashMap::new(),
        }
    }
}

#[async_trait]
impl EventStore for InMemoryEventStore {
    type AggregateId = String;

    async fn save_events(
        &mut self,
        aggregate_id: Self::AggregateId,
        events: &[Event],
    ) -> Result<(), CqrsError> {
        if let Some(current_events) = self.events.get_mut(&aggregate_id) {
            current_events.extend(events.to_vec());
        } else {
            self.events.insert(aggregate_id.to_string(), events.to_vec());
        };
        Ok(())
    }

    async fn load_events(
        &self,
        aggregate_id: Self::AggregateId,
    ) -> Result<Vec<Event>, CqrsError> {
        if let Some(events) = self.events.get(&aggregate_id) {
            Ok(events.to_vec())
        } else {
            Err(CqrsError::new(format!(
                "No events for aggregate id `{}`",
                aggregate_id
            )))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();
    let consumers: Vec<Box<dyn EventConsumer>> = vec![
        Box::new(PrintEventConsumer {}),
        Box::new(AnotherEventConsumer {}),
    ];

    let mut dispatcher: SimpleDispatcher<CounterState, InMemoryEventStore> =
        SimpleDispatcher::new(store, consumers);

    let result = dispatcher
        .execute("12345".to_string(), CounterCommand::Increment(10))
        .await?;
    assert_eq!(result.count, 10);
    println!("MAIN: Counter state: {}", result.count);

    let result = dispatcher
        .execute("12345".to_string(), CounterCommand::Decrement(3))
        .await?;
    assert_eq!(result.count, 7);
    println!("MAIN: Counter state: {}", result.count);

    if let Err(msg) = dispatcher
        .execute("12345".to_string(), CounterCommand::Decrement(10))
        .await
    {
        println!("MAIN: {:?}", msg);
    }

    println!("MAIN: Counter state: {}", result.count);

    Ok(())
}
