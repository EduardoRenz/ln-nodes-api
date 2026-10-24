use std::future::Future;

use crate::application::import_nodes::{ImportError, ImportOutcome};
use crate::logging::file_sink::SinkError;

/// Failure while persisting an import-run record.
///
/// A sum of per-backend errors: the file backend contributes [`SinkError`],
/// and future backends (Datadog, Sentry, ...) add their own variants. The
/// message is transparent so the source's text — which already carries the
/// path — reaches the console unchanged.
#[derive(Debug, thiserror::Error)]
pub enum LoggerError {
    #[error(transparent)]
    Sink(#[from] SinkError),
}

/// Destination for one-line import-run records.
///
/// The application depends on this interface, not on a particular backend.
/// Implementations live in the sibling modules and are selected at runtime by
/// the factory module. This is an audit sink, not a leveled logger: every run
/// is recorded regardless of `RUST_LOG`, because dropping a record would mean
/// losing audit exactly when the process is misbehaving. Diagnostic messages
/// stay with `tracing`.
pub trait RunLogger: Send + Sync {
    /// Persist the outcome of a single import run.
    fn record(
        &self,
        outcome: &Result<ImportOutcome, ImportError>,
    ) -> impl Future<Output = Result<(), LoggerError>> + Send;

    /// Record the run and degrade a destination failure to a warning.
    ///
    /// Shared by the scheduler and the one-shot `import` command so the
    /// "logging never interrupts an import" policy lives in exactly one
    /// place.
    fn record_or_warn(
        &self,
        outcome: &Result<ImportOutcome, ImportError>,
    ) -> impl Future<Output = ()> + Send {
        async move {
            if let Err(error) = self.record(outcome).await {
                tracing::warn!(%error, "failed to record import run");
            }
        }
    }
}
