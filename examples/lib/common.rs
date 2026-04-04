#![allow(dead_code)]

// Common code shared in the examples to avoid repetitions and focus on the core concepts

use std::{collections::HashMap, sync::Mutex};

use mini_cqrs_es::{CqrsError, Event, EventStore, Uuid};

// Event Store
pub struct InMemoryEventStore {
    events: Mutex<HashMap<Uuid, Vec<Event>>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        InMemoryEventStore {
            events: Mutex::new(HashMap::new()),
        }
    }
}

impl EventStore for InMemoryEventStore {
    async fn save_events(
        &self,
        aggregate_id: Uuid,
        events: &[Event],
        expected_version: u64,
    ) -> Result<(), CqrsError> {
        let mut store = self.events.lock().unwrap();
        let current = store.entry(aggregate_id).or_default();
        let actual_version = current.last().map(|e| e.version).unwrap_or(0);

        if actual_version != expected_version {
            return Err(CqrsError::Conflict {
                expected_version,
                actual_version,
            });
        }

        current.extend(events.to_vec());
        Ok(())
    }

    async fn load_events(&self, aggregate_id: Uuid) -> Result<(Vec<Event>, u64), CqrsError> {
        let store = self.events.lock().unwrap();
        if let Some(events) = store.get(&aggregate_id) {
            let version = events.last().map(|e| e.version).unwrap_or(0);
            Ok((events.to_vec(), version))
        } else {
            Ok((vec![], 0))
        }
    }
}
