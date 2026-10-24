//! File backend for import-run records: [`FileSink`] mechanics plus the
//! domain line format.

use std::path::PathBuf;

use chrono::{DateTime, SecondsFormat, Utc};

use crate::application::import_nodes::{ImportError, ImportOutcome};
use crate::config::RepositoryType;
use crate::logging::file_sink::FileSink;
use crate::logging::run_logger::{LoggerError, RunLogger};

/// Writes one line per import run to a date-partitioned file.
///
/// The sink owns *where* the line goes (directory and filename pattern) and
/// this type owns *what* the line says — the canonical audit format stays
/// here because a future non-file backend would map fields instead of
/// rendering strings.
pub struct FileRunLogger {
    sink: FileSink,
    persistence: RepositoryType,
}

impl FileRunLogger {
    /// Logger writing `YYYY-MM-DD.log` under `dir`, tagging every line with
    /// the persistence backend the runs write to.
    pub fn new(dir: PathBuf, persistence: RepositoryType) -> Self {
        Self {
            // `{date}.log` — the established filename; origin stays out of
            // it because an import run is the only writer.
            sink: FileSink::new("import").in_dir(dir).pattern("{date}.log"),
            persistence,
        }
    }

    /// [`RunLogger::record`] with an explicit timestamp — the entry point
    /// tests use to pin the date partition.
    fn record_at(
        &self,
        timestamp: DateTime<Utc>,
        outcome: &Result<ImportOutcome, ImportError>,
    ) -> Result<(), LoggerError> {
        let line = format_run(timestamp, self.persistence, outcome);
        Ok(self.sink.append_at(timestamp, &line)?)
    }
}

impl RunLogger for FileRunLogger {
    async fn record(
        &self,
        outcome: &Result<ImportOutcome, ImportError>,
    ) -> Result<(), LoggerError> {
        self.record_at(Utc::now(), outcome)
    }
}

/// Renders one run as a single line:
/// `<RFC 3339 UTC> <outcome> persistence=<backend> [count=..., new=...]`.
fn format_run(
    timestamp: DateTime<Utc>,
    persistence: RepositoryType,
    result: &Result<ImportOutcome, ImportError>,
) -> String {
    let stamp = timestamp.to_rfc3339_opts(SecondsFormat::Secs, true);
    let persistence = persistence.as_str();

    match result {
        Ok(ImportOutcome::Imported { count, new }) => {
            format!("{stamp} imported persistence={persistence} count={count} new={new}")
        }
        Ok(ImportOutcome::SkippedEmpty) => {
            format!("{stamp} skipped persistence={persistence} mempool returned no valid nodes")
        }
        Err(error) => format!("{stamp} error persistence={persistence} {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateways::nodes_gateway::GatewayError;
    use crate::repositories::nodes_repository::RepositoryError;
    use chrono::TimeZone;
    use std::path::Path;

    fn stamp(offset_secs: i64) -> DateTime<Utc> {
        // 2026-10-14T17:46:40Z, plus one day per test variant.
        Utc.timestamp_opt(1_792_000_000 + offset_secs, 0)
            .single()
            .expect("timestamp")
    }

    fn read(dir: &Path, date: &str) -> String {
        std::fs::read_to_string(dir.join(format!("{date}.log"))).expect("log file")
    }

    fn logger(dir: &Path, persistence: RepositoryType) -> FileRunLogger {
        FileRunLogger::new(dir.to_path_buf(), persistence)
    }

    #[test]
    fn creates_missing_directory_and_appends_one_line_per_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("logs");
        let log = logger(&nested, RepositoryType::Postgres);

        log.record_at(
            stamp(0),
            &Ok(ImportOutcome::Imported {
                count: 357,
                new: 12,
            }),
        )
        .expect("first run");
        log.record_at(
            stamp(0),
            &Ok(ImportOutcome::Imported { count: 358, new: 0 }),
        )
        .expect("second run");

        let contents = read(&nested, "2026-10-14");
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(
            lines,
            vec![
                "2026-10-14T17:46:40Z imported persistence=postgres count=357 new=12",
                "2026-10-14T17:46:40Z imported persistence=postgres count=358 new=0",
            ]
        );
    }

    #[test]
    fn partitions_runs_into_files_by_date() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log = logger(dir.path(), RepositoryType::Sqlite);

        log.record_at(stamp(0), &Ok(ImportOutcome::Imported { count: 1, new: 1 }))
            .expect("first day");
        log.record_at(
            stamp(86_400),
            &Ok(ImportOutcome::Imported { count: 2, new: 1 }),
        )
        .expect("second day");

        assert!(dir.path().join("2026-10-14.log").is_file());
        assert!(dir.path().join("2026-10-15.log").is_file());
        assert!(read(dir.path(), "2026-10-14").ends_with("count=1 new=1\n"));
        assert!(read(dir.path(), "2026-10-15").ends_with("count=2 new=1\n"));
    }

    #[test]
    fn records_skipped_runs_with_the_configured_backend() {
        let dir = tempfile::tempdir().expect("temp dir");

        logger(dir.path(), RepositoryType::InMemory)
            .record_at(stamp(0), &Ok(ImportOutcome::SkippedEmpty))
            .expect("skipped run");

        assert_eq!(
            read(dir.path(), "2026-10-14"),
            "2026-10-14T17:46:40Z skipped persistence=in_memory mempool returned no valid nodes\n"
        );
    }

    #[test]
    fn records_failed_runs_with_the_full_cause() {
        let dir = tempfile::tempdir().expect("temp dir");
        let result: Result<ImportOutcome, ImportError> =
            Err(ImportError::Gateway(GatewayError::Request));

        logger(dir.path(), RepositoryType::Postgres)
            .record_at(stamp(0), &result)
            .expect("failed run");

        // The line carries the whole chain, so the file alone explains the
        // failure without the console output.
        assert_eq!(
            read(dir.path(), "2026-10-14"),
            "2026-10-14T17:46:40Z error persistence=postgres failed to fetch nodes: mempool request failed\n"
        );
    }

    #[test]
    fn records_repository_failures_with_the_database_cause() {
        let dir = tempfile::tempdir().expect("temp dir");
        let result: Result<ImportOutcome, ImportError> =
            Err(ImportError::Repository(RepositoryError::Unavailable(
                "error communicating with database: connection refused".to_string(),
            )));

        logger(dir.path(), RepositoryType::Postgres)
            .record_at(stamp(0), &result)
            .expect("failed run");

        let contents = read(dir.path(), "2026-10-14");
        assert!(
            contents.contains(
                "error persistence=postgres repository operation failed: \
                 error communicating with database: connection refused"
            ),
            "unexpected contents: {contents}"
        );
    }

    #[tokio::test]
    async fn record_uses_the_current_utc_date() {
        let dir = tempfile::tempdir().expect("temp dir");

        logger(dir.path(), RepositoryType::Sqlite)
            .record(&Ok(ImportOutcome::Imported { count: 1, new: 1 }))
            .await
            .expect("record");

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let contents = read(dir.path(), &today);
        assert!(
            contents.starts_with(&format!("{today}T"))
                && contents.ends_with("persistence=sqlite count=1 new=1\n"),
            "unexpected contents: {contents}"
        );
    }
}
