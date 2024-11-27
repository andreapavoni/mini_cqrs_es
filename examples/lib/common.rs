#![allow(dead_code)]

// Common code shared in the examples to avoid repetitions and focus on the core concepts

use std::collections::HashMap;

use anyhow::{anyhow, Error};
use async_trait::async_trait;

use mini_cqrs_es::{Event, EventStore, Uuid};

// Event Store
#[derive(Clone)]
pub struct InMemoryEventStore {
    events: HashMap<Uuid, Vec<Event>>,
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
    async fn save_events(&mut self, aggregate_id: Uuid, events: &[Event]) -> Result<(), Error> {
        if let Some(current_events) = self.events.get_mut(&aggregate_id) {
            current_events.extend(events.to_vec());
        } else {
            self.events.insert(aggregate_id, events.into());
        };

        Ok(())
    }

    async fn load_events(&self, aggregate_id: Uuid) -> Result<Vec<Event>, Error> {
        if let Some(events) = self.events.get(&aggregate_id) {
            Ok(events.to_vec())
        } else {
            Err(anyhow!("No events for aggregate id `{}`", aggregate_id))
        }
    }
}
