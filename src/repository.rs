/// A trait representing a repository for event-sourced aggregates.
///
/// Repositories provide an abstraction over storage mechanisms for event-sourced aggregates.
/// They allow you to load and store aggregates and their associated events.
///
/// Implement this trait to define custom repository logic for your event-sourced aggregates.
///
/// # Example
///
/// ```rust
/// use mini_cqrs::Aggregate;
///
/// struct YourAggregateRepository;
///
/// impl Repository for YourAggregateRepository {
///     // Implement repository methods here.
/// }
/// ```
pub trait Repository: Send + Sync + Clone {}
