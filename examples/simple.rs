/// # Mini CQRS Example: Basic Counter
///
/// This basic example demonstrates how to use the `mini_cqrs` framework. It features a simple
/// Counter aggregate that responds to increment and decrement commands, emitting corresponding
/// events. Two dummy consumers are used to print the received events.
///
/// Here, we only use a dispatcher, without using a full CQRS wrapper or queries on read models.
///
/// ## Usage
///
/// ```sh
/// cargo run --example simple
/// ```

use async_trait::async_trait;

use mini_cqrs::{
    wrap_event, Aggregate, CqrsError, Dispatcher, Event, EventConsumer, EventPayload,
    SimpleDispatcher, event_consumers_group, EventConsumersGroup,
};
use serde::{Deserialize, Serialize};

#[path = "lib/common.rs"]
mod common;
use common::InMemoryEventStore;

// Aggregate
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
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
            CounterEvent::Incremented(..) => "Incremented".to_string(),
            CounterEvent::Decremented(..) => "Decremented".to_string(),
        }
    }
}

wrap_event!(CounterEvent);

// Consumer
#[derive(Debug, Clone)]
pub struct PrintEventConsumer {}

#[async_trait]
impl EventConsumer for PrintEventConsumer {
    async fn process(&mut self, event: Event) {
        println!("C: Consuming event: {:#?}", event);
    }
}

#[derive(Debug, Clone)]
pub struct AnotherEventConsumer {}

#[async_trait]
impl EventConsumer for AnotherEventConsumer {
    async fn process(&mut self, event: Event) {
        println!("C: Consuming event on another consumer: {:#?}", event);
    }
}

event_consumers_group! {
    SimpleEventConsumers {
        Print => PrintEventConsumer,
        Another => AnotherEventConsumer,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();

    let consumers = vec![
        SimpleEventConsumers::Print(PrintEventConsumer {}),
        SimpleEventConsumers::Another(AnotherEventConsumer {}),
    ];

    let mut dispatcher: SimpleDispatcher<CounterState, InMemoryEventStore, SimpleEventConsumers> =
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
