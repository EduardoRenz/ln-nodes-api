use crate::config::{Config, LogBackend};
use crate::logging::any_run_logger::AnyRunLogger;
use crate::logging::file_run_logger::FileRunLogger;
use crate::logging::run_logger::RunLogger;

/// Creates `RunLogger` implementations.
///
/// The factory only builds and returns the implementation. It never
/// implements the logging behavior itself, and it is never a global
/// accessor. Construction is infallible today — the file backend has no
/// failure mode at build time; a future HTTP-backed backend that validates
/// credentials can widen the return type to `Result` without touching
/// callers beyond the `?`.
pub trait RunLoggerFactory: Send + Sync {
    type Logger: RunLogger + 'static;

    fn create(&self, config: &Config) -> Self::Logger;
}

/// Builds the backend named by `LOG_BACKEND`, wiring the configuration that
/// backend needs (the file backend takes `LOG_DIR` and the persistence label
/// the runs are tagged with).
pub struct ConfiguredRunLoggerFactory;

impl RunLoggerFactory for ConfiguredRunLoggerFactory {
    type Logger = AnyRunLogger;

    fn create(&self, config: &Config) -> Self::Logger {
        match config.log_backend {
            LogBackend::File => AnyRunLogger::File(FileRunLogger::new(
                config.log_dir.clone(),
                config.repository_type,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::import_nodes::ImportOutcome;

    fn config_with(extra: &[(&str, &str)]) -> Config {
        let mut values = vec![
            ("PERSISTENCE", "SQLITE"),
            ("DATABASE_URL", "sqlite://unused?mode=rwc"),
        ];
        values.extend_from_slice(extra);

        Config::from_lookup(|key| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_string())
        })
        .expect("valid config")
    }

    #[test]
    fn defaults_to_the_file_backend_when_unset() {
        let config = config_with(&[]);

        assert_eq!(config.log_backend, LogBackend::File);
        let logger = ConfiguredRunLoggerFactory.create(&config);
        assert!(matches!(logger, AnyRunLogger::File(_)));
    }

    #[test]
    fn selects_the_backend_named_by_configuration() {
        let config = config_with(&[("LOG_BACKEND", "FILE")]);

        let logger = ConfiguredRunLoggerFactory.create(&config);

        assert!(matches!(logger, AnyRunLogger::File(_)));
    }

    #[tokio::test]
    async fn wires_the_configured_directory_into_the_sink() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = config_with(&[("LOG_DIR", &dir.path().display().to_string())]);

        let logger = ConfiguredRunLoggerFactory.create(&config);
        logger
            .record(&Ok(ImportOutcome::Imported { count: 1, new: 1 }))
            .await
            .expect("record");

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let contents =
            std::fs::read_to_string(dir.path().join(format!("{today}.log"))).expect("log file");
        assert!(
            contents.contains("persistence=sqlite count=1 new=1"),
            "unexpected contents: {contents}"
        );
    }
}
