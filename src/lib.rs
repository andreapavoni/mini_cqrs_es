pub trait Aggregate {
    type Command;
    type Event;

    fn handle(&self, command: &Self::Command) -> Vec<Self::Event>;
    fn apply(&mut self, event: &Self::Event);
}

