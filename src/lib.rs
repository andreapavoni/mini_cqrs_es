mod error;
mod aggregate;
mod consumer;
mod event_store;
mod query;
mod dispatcher;
mod cqrs;

pub use error::CqrsError;
pub use aggregate::Aggregate;
pub use consumer::EventConsumer;
pub use event_store::EventStore;
pub use query::{Query, ModelReader};
pub use dispatcher::{Dispatcher, SimpleDispatcher};
pub use cqrs::Cqrs;

pub trait Repository: Send + Sync + Clone {}
