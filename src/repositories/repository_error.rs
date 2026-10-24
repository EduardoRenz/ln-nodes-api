/// Error vocabulary shared by every persistence port.
///
/// Both variants describe conditions of the persistence boundary itself, not
/// of a particular entity: any stored aggregate can fail to reach the database
/// (`Unavailable`) or come back as rows that cannot be mapped to domain values
/// (`InvalidData`). The type is therefore defined once here rather than being
/// repeated in each port module.
///
/// A port that needs richer vocabulary (`NotFound`, `Conflict`, …) defines its
/// own error enum instead of widening this one. Variants are only added here
/// when every persistence port can exhibit them.
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
