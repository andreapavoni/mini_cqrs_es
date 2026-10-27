//! Backend-independent persistence through Toasty. Select a driver feature in
//! the application, connect to its URL, and explicitly initialize fresh schemas.
mod event_log;
mod snapshots;
pub use event_log::{EventLog, EventRecord};
pub use snapshots::{SnapshotCache, SnapshotRecord};
