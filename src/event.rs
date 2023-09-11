use serde::{de::DeserializeOwned, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Event {
    pub id: String,
    pub event_type: String,
    pub aggregate_id: String,
    payload: serde_json::Value,
    pub version: u64,
}

impl Event {
    pub fn new<T: EventPayload>(
        payload: T,
        version: Option<u64>,
    ) -> Self {
        let version = version.unwrap_or(1);

        Self {
            id: Uuid::new_v4().to_string(),
            event_type: payload.name(),
            aggregate_id: payload.aggregate_id(),
            payload: serde_json::to_value(payload).unwrap(),
            version,
        }
    }

    pub fn get_payload<T: EventPayload>(&self) -> T {
        serde_json::from_value(self.payload.clone()).unwrap()
    }
}

#[macro_export]
macro_rules! act_as_event {
    ($evt: ident) => {
        impl From<Event> for $evt {
            fn from(evt: Event) -> Self {
                evt.get_payload::<$evt>()
            }
        }

        impl Into<Event> for $evt {
            fn into(self) -> Event {
                Event::new(self, None)
            }
        }
    };
}

#[allow(unused)]
pub(crate) use act_as_event;

pub trait EventPayload<Evt = Self>: Serialize + DeserializeOwned + Clone + ToString {
    fn aggregate_id(&self) -> String;

    fn name(&self) -> String {
        self.to_string()
    }
}
