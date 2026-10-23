use std::future::Future;

use crate::domain::Node;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GatewayError {
    #[error("mempool request failed")]
    Request,
    #[error("mempool returned HTTP status {0}")]
    HttpStatus(u16),
    #[error("mempool returned an invalid payload")]
    InvalidPayload,
}

/// External boundary for Lightning node data.
///
/// The application depends on this interface, not on the mempool.space API.
pub trait NodesGateway: Send + Sync {
    fn fetch(&self) -> impl Future<Output = Result<Vec<Node>, GatewayError>> + Send;
}
