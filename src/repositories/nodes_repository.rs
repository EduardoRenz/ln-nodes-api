use std::future::Future;

use crate::domain::Node;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoryError {
    /// The database could not serve the operation. The payload is the
    /// concrete cause (a string, so the port stays framework-free) and is
    /// the *entire* display: every caller already adds its own context
    /// ("failed to list nodes: …", "repository operation failed: …"), so
    /// repeating a generic prefix here would only make chains noisy.
    #[error("{0}")]
    Unavailable(String),
    #[error("database returned invalid domain data")]
    InvalidData,
}

/// Persistence boundary for Lightning nodes.
///
/// The application depends on this interface, not on a concrete database
/// implementation. Implementations live in the sibling modules.
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
