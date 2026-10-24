use crate::application::import_nodes::{ImportError, ImportOutcome};
use crate::logging::file_run_logger::FileRunLogger;
use crate::logging::run_logger::{LoggerError, RunLogger};

/// Runtime-selectable `RunLogger` wrapper.
///
/// The factory module decides which backend to build from configuration;
/// this enum is the result of that choice. It implements the port by
/// delegating to the selected variant and contains no logging logic of its
/// own. Future backends (Datadog, Sentry, ...) add a variant here and a
/// arm in the factory — no call site changes.
pub enum AnyRunLogger {
    File(FileRunLogger),
}

impl RunLogger for AnyRunLogger {
    async fn record(
        &self,
        outcome: &Result<ImportOutcome, ImportError>,
    ) -> Result<(), LoggerError> {
        match self {
            Self::File(logger) => logger.record(outcome).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RepositoryType;
    use crate::gateways::nodes_gateway::GatewayError;

    #[tokio::test]
    async fn delegates_to_the_file_variant() {
        let dir = tempfile::tempdir().expect("temp dir");
        let logger = AnyRunLogger::File(FileRunLogger::new(
            dir.path().to_path_buf(),
            RepositoryType::Sqlite,
        ));

        logger
            .record(&Ok(ImportOutcome::Imported { count: 1, new: 1 }))
            .await
            .expect("record");

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let contents =
            std::fs::read_to_string(dir.path().join(format!("{today}.log"))).expect("log file");
        assert!(contents.ends_with("persistence=sqlite count=1 new=1\n"));
    }

    #[tokio::test]
    async fn surfaces_errors_from_the_selected_variant() {
        // A path under a regular file: create_dir_all fails inside the
        // backend and the error must reach the caller unchanged.
        let dir = tempfile::tempdir().expect("temp dir");
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "not a directory").expect("blocker file");
        let logger =
            AnyRunLogger::File(FileRunLogger::new(blocker.clone(), RepositoryType::Sqlite));

        let result = logger.record(&Ok(ImportOutcome::SkippedEmpty)).await;

        let error = result.expect_err("must fail");
        assert!(error.to_string().contains("failed to write log at"));
    }

    #[tokio::test]
    async fn record_or_warn_never_fails_even_when_the_sink_does() {
        let dir = tempfile::tempdir().expect("temp dir");
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "not a directory").expect("blocker file");
        let logger = AnyRunLogger::File(FileRunLogger::new(blocker, RepositoryType::Sqlite));

        // Must return normally: a broken destination degrades to a warning.
        logger
            .record_or_warn(&Err(ImportError::Gateway(GatewayError::Request)))
            .await;
    }
}
