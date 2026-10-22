#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("public key must not be empty")]
    EmptyPublicKey,
    #[error("missing capacity")]
    MissingCapacity,
    #[error("capacity must be non-negative: {0}")]
    NegativeCapacity(i64),
    #[error("missing first seen timestamp")]
    MissingFirstSeen,
    #[error("invalid unix timestamp: {0}")]
    InvalidTimestamp(i64),
    #[error("duplicate public key: {0}")]
    DuplicatePublicKey(String),
}
