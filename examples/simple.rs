/// A basic example to show how to use the `mini_cqrs` framework.
/// Here we have a Counter aggregate which accepts increment/decrement commands and emits
/// incremented/decremented events. Two consumer will just print the events they receive.
use std::collections::HashMap;

use async_trait::async_trait;

use mini_cqrs::{Aggregate, CqrsError, Dispatcher, EventConsumer, EventStore, SimpleDispatcher};

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

    async fn handle(&self, command: &Self::Command) -> Result<Vec<Self::Event>, CqrsError> {
        match command {
            CounterCommand::Increment(amount) => Ok(vec![CounterEvent::Incremented(*amount)]),
            CounterCommand::Decrement(amount) => {
                if self.count < *amount {
                    return Err(CqrsError::new(format!(
                        "COMMANDERROR: Decrement amount {} is greater than current count {}",
                        amount, self.count
                    )));
                }
                Ok(vec![CounterEvent::Decremented(*amount)])
            }
        }
    }

    fn apply(&mut self, event: &Self::Event) {
        match event {
            CounterEvent::Incremented(amount) => self.count += amount,
            CounterEvent::Decremented(amount) => self.count -= amount,
        };
    }

    fn aggregate_id(&self) -> &str {
        self.id.as_str()
    }
}

// Commands
#[derive(Debug, PartialEq, Clone)]
enum CounterCommand {
    Increment(u32),
    Decrement(u32),
}

// Events
#[derive(Debug, PartialEq, Clone)]
enum CounterEvent {
    Incremented(u32),
    Decremented(u32),
}

// Consumer
struct PrintEventConsumer {}

#[async_trait]
impl EventConsumer for PrintEventConsumer {
    type Event = CounterEvent;

    async fn process<'a>(&self, event: &'a Self::Event) {
        println!("C: Consuming event: {:?}", event);
    }
}

struct AnotherEventConsumer {}

#[async_trait]
impl EventConsumer for AnotherEventConsumer {
    type Event = CounterEvent;

    async fn process<'a>(&self, event: &'a Self::Event) {
        println!("C: Consuming event on another consumer: {:?}", event);
    }
}

// Event Store

struct InMemoryEventStore {
    events: HashMap<String, Vec<CounterEvent>>,
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
    type Event = CounterEvent;

    async fn save_events(
        &mut self,
        aggregate_id: &str,
        events: &Vec<Self::Event>,
    ) -> Result<(), CqrsError> {
        if let Some(current_events) = self.events.get_mut(aggregate_id) {
            current_events.extend(events.clone());
        } else {
            self.events.insert(aggregate_id.to_string(), events.clone());
        };
        Ok(())
    }

    async fn load_events(&self, aggregate_id: &str) -> Result<Vec<Self::Event>, CqrsError> {
        if let Some(events) = self.events.get(aggregate_id) {
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
    let consumers: Vec<Box<dyn EventConsumer<Event = CounterEvent>>> = vec![
        Box::new(PrintEventConsumer {}),
        Box::new(AnotherEventConsumer {}),
    ];

    let mut dispatcher: SimpleDispatcher<CounterState, InMemoryEventStore> =
        SimpleDispatcher::new(store, consumers);

    let result = dispatcher
        .execute("12345", &CounterCommand::Increment(10))
        .await?;
    assert_eq!(result.count, 10);
    println!("MAIN: Counter state: {}", result.count);

    let result = dispatcher
        .execute("12345", &CounterCommand::Decrement(3))
        .await?;
    assert_eq!(result.count, 7);
    println!("MAIN: Counter state: {}", result.count);

    if let Err(msg) = dispatcher
        .execute("12345", &CounterCommand::Decrement(10))
        .await
    {
        println!("MAIN: {:?}", msg);
    }

    println!("MAIN: Counter state: {}", result.count);

    Ok(())
}
