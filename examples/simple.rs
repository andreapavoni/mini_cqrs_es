
use gpt::Aggregate;


struct CounterState {
    count: u32,
}

impl CounterState {
    pub fn new(count: u32) -> Self {
        CounterState { count }
    }
}

enum CounterCommand {
    Increment,
    Decrement,
}

#[derive(PartialEq)]
enum CounterEvent {
    Incremented,
    Decremented,
}

impl Aggregate for CounterState {
    type Command = CounterCommand;
    type Event = CounterEvent;

    fn handle(&self, command: &Self::Command) -> Vec<Self::Event> {
        match command {
            CounterCommand::Increment => vec![CounterEvent::Incremented],
            CounterCommand::Decrement => vec![CounterEvent::Decremented],
        }
    }


    fn apply(&mut self, event: &Self::Event) {
        match event {
            CounterEvent::Incremented => self.count += 1,
            CounterEvent::Decremented => self.count -= 1,
        };
    }
}

fn main() {
    let mut counter = CounterState::new(0);

    println!("Counter initial: {}", counter.count);

    let events = counter.handle(&CounterCommand::Increment);
    assert!(events.contains(&CounterEvent::Incremented));
    for e in events.into_iter() {
        counter.apply(&e);
    }
    assert_eq!(counter.count, 1);
    println!("Counter increment: {}", counter.count);

    let events = counter.handle(&CounterCommand::Decrement);
    assert!(events.contains(&CounterEvent::Decremented));
    for e in events.into_iter() {
        counter.apply(&e);
    }
    assert_eq!(counter.count, 0);
    println!("Counter decrement: {}", counter.count);
}
