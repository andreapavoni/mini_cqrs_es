use std::fmt::{Debug, Display, Formatter};

/// An error that can occur in a CQRS application.
#[derive(Debug)]
pub struct CqrsError(String);

impl CqrsError {
    /// Creates a new CqrsError.
    pub fn new(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for CqrsError {
    fn from(msg: &str) -> Self {
        Self(msg.to_string())
    }
}

impl Display for CqrsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CqrsError {}
