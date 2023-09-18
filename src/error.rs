use std::fmt::{Debug, Display, Formatter};

/// An error type for CQRS-related errors.
///
/// CqrsError is used to represent various errors that can occur within the CQRS framework.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::CqrsError;
///
/// fn your_function() -> Result<(), CqrsError> {
///     // ...
///     // Return an error if something goes wrong.
///     Err(CqrsError::new("An error occurred.".to_string()))
/// }
/// ```
#[derive(Debug)]
pub struct CqrsError(String);

impl CqrsError {
    /// Creates a new CqrsError instance with the specified error message.
    ///
    /// # Parameters
    ///
    /// - `message`: A descriptive error message.
    ///
    /// # Returns
    ///
    /// A new CqrsError instance.
    pub fn new(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for CqrsError {
    /// Converts a string slice into a CqrsError instance.
    ///
    /// # Parameters
    ///
    /// - `msg`: A string slice representing the error message.
    ///
    /// # Returns
    ///
    /// A CqrsError instance with the provided error message.
    fn from(msg: &str) -> Self {
        Self(msg.to_string())
    }
}

impl Display for CqrsError {
    /// Formats the CqrsError for display.
    ///
    /// This implementation allows the error message to be displayed using the `write!` and `println!` macros.
    ///
    /// # Parameters
    ///
    /// - `f`: A mutable reference to the formatter.
    ///
    /// # Returns
    ///
    /// A result indicating success or an error if formatting fails.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CqrsError {}
