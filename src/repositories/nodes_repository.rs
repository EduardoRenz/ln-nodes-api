use std::future::Future;

use crate::domain::Node;

pub use crate::repositories::repository_error::RepositoryError;

/// Persistence boundary for Lightning nodes.
///
/// The application depends on this interface, not on a concrete database
/// implementation. Implementations live in the sibling modules. The port
/// re-exports [`RepositoryError`] so importing from this module keeps the
/// port's vocabulary in one place.
pub trait NodesRepository: Send + Sync {
    /// Replace the current snapshot with the supplied nodes.
    fn save_snapshot<'a>(
        &'a self,
        nodes: &'a [Node],
    ) -> impl Future<Output = Result<(), RepositoryError>> + Send + 'a;

    /// Return the current snapshot ordered by the source ranking.
    fn list(&self) -> impl Future<Output = Result<Vec<Node>, RepositoryError>> + Send;

    /// Check whether the persistence backend is reachable.
    fn ping(&self) -> impl Future<Output = Result<(), RepositoryError>> + Send;
}
