#![allow(dead_code)]

// Common code shared in the examples to avoid repetitions and focus on the core concepts

use std::{collections::HashMap, sync::Mutex};

use mini_cqrs_es::{CqrsError, EventStore, NewEvent, StoredEvent};

// Event Store
pub struct InMemoryEventStore {
    events: Mutex<HashMap<String, Vec<StoredEvent>>>,
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
        aggregate_type: &str,
        aggregate_id: &str,
        events: &[NewEvent],
        expected_version: u64,
    ) -> Result<Vec<StoredEvent>, CqrsError> {
        let mut store = self.events.lock().unwrap();
        let current = store.entry(aggregate_id.to_string()).or_default();
        let actual_version = current.last().map(|e| e.version).unwrap_or(0);

        if actual_version != expected_version {
            return Err(CqrsError::Conflict {
                expected_version,
                actual_version,
            });
        }

        let mut persisted = Vec::with_capacity(events.len());
        for (i, event) in events.iter().enumerate() {
            let stored = StoredEvent {
                id: format!("{aggregate_id}-{}", actual_version + i as u64 + 1),
                aggregate_id: aggregate_id.to_string(),
                aggregate_type: aggregate_type.to_string(),
                version: actual_version + i as u64 + 1,
                event_type: event.event_type.clone(),
                payload: event.payload.clone(),
                metadata: event.metadata.clone(),
                global_sequence: None,
                timestamp: event.timestamp,
            };
            persisted.push(stored);
        }

        if persisted.is_empty() {
            return Ok(vec![]);
        }

        current.extend(persisted.clone());
        Ok(persisted)
    }

    async fn load_events(
        &self,
        _aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<(Vec<StoredEvent>, u64), CqrsError> {
        let store = self.events.lock().unwrap();
        if let Some(events) = store.get(aggregate_id) {
            let version = events.last().map(|e| e.version).unwrap_or(0);
            Ok((events.to_vec(), version))
        } else {
            Ok((vec![], 0))
        }
    }
}
