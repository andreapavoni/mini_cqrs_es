#![allow(dead_code)]

// Common code shared in the examples to avoid repetitions and focus on the core concepts

use std::collections::HashMap;

use async_trait::async_trait;

use mini_cqrs::{CqrsError, Event, EventStore};

// Event Store
pub struct InMemoryEventStore {
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
            self.events.insert(aggregate_id.to_string(), events.into());
        };
        Ok(())
    }

    async fn load_events(&self, aggregate_id: Self::AggregateId) -> Result<Vec<Event>, CqrsError> {
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
