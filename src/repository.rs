/// A marker trait for repository types used in read model projections.
///
/// This is a convenience trait to ensure repository types are thread-safe.
pub trait Repository: Send + Sync {}
