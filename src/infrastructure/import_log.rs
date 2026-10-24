//! Persists one line per import run, partitioned by UTC date.
//!
//! The scheduler calls [`record`] after every run, including failed ones, so
//! the directory answers "what happened to the imports on a given day?"
//! without digging through console output. Files are append-only
//! (`YYYY-MM-DD.log`); a write failure is reported to the caller and never
//! interrupts the import loop.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};

use crate::application::import_nodes::{ImportError, ImportOutcome};

/// Appends the outcome of a single import run to the log file for
/// `timestamp`'s UTC date, creating `dir` when missing.
pub fn record(
    dir: &Path,
    timestamp: DateTime<Utc>,
    result: &Result<ImportOutcome, ImportError>,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;

    let path = dir.join(format!("{}.log", timestamp.format("%Y-%m-%d")));
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;

    writeln!(file, "{}", format_run(timestamp, result))
}

/// [`record`] with the current UTC time — the entry point shared by the
/// scheduler and the one-shot `import` command.
pub fn record_now(dir: &Path, result: &Result<ImportOutcome, ImportError>) -> io::Result<()> {
    record(dir, Utc::now(), result)
}

/// Renders one run as a single line: `<RFC 3339 UTC> <outcome>`.
fn format_run(timestamp: DateTime<Utc>, result: &Result<ImportOutcome, ImportError>) -> String {
    let stamp = timestamp.to_rfc3339_opts(SecondsFormat::Secs, true);

    match result {
        Ok(ImportOutcome::Imported { count }) => {
            format!("{stamp} imported count={count}")
        }
        Ok(ImportOutcome::SkippedEmpty) => {
            format!("{stamp} skipped mempool returned no valid nodes")
        }
        Err(error) => format!("{stamp} error {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateways::nodes_gateway::GatewayError;
    use chrono::TimeZone;

    fn stamp(offset_secs: i64) -> DateTime<Utc> {
        // 2026-10-14T17:46:40Z, plus one day per test variant.
        Utc.timestamp_opt(1_792_000_000 + offset_secs, 0)
            .single()
            .expect("timestamp")
    }

    fn read(dir: &Path, date: &str) -> String {
        fs::read_to_string(dir.join(format!("{date}.log"))).expect("log file")
    }

    #[test]
    fn creates_missing_directory_and_appends_one_line_per_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("logs");
        let today = stamp(0);

        record(&nested, today, &Ok(ImportOutcome::Imported { count: 357 })).expect("first run");
        record(&nested, today, &Ok(ImportOutcome::Imported { count: 358 })).expect("second run");

        let contents = read(&nested, "2026-10-14");
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(
            lines,
            vec![
                "2026-10-14T17:46:40Z imported count=357",
                "2026-10-14T17:46:40Z imported count=358",
            ]
        );
    }

    #[test]
    fn partitions_runs_into_files_by_date() {
        let dir = tempfile::tempdir().expect("temp dir");

        record(
            dir.path(),
            stamp(0),
            &Ok(ImportOutcome::Imported { count: 1 }),
        )
        .expect("first day");
        record(
            dir.path(),
            stamp(86_400),
            &Ok(ImportOutcome::Imported { count: 2 }),
        )
        .expect("second day");

        assert!(dir.path().join("2026-10-14.log").is_file());
        assert!(dir.path().join("2026-10-15.log").is_file());
        assert!(read(dir.path(), "2026-10-14").ends_with("count=1\n"));
        assert!(read(dir.path(), "2026-10-15").ends_with("count=2\n"));
    }

    #[test]
    fn records_skipped_runs() {
        let dir = tempfile::tempdir().expect("temp dir");

        record(dir.path(), stamp(0), &Ok(ImportOutcome::SkippedEmpty)).expect("skipped run");

        assert_eq!(
            read(dir.path(), "2026-10-14"),
            "2026-10-14T17:46:40Z skipped mempool returned no valid nodes\n"
        );
    }

    #[test]
    fn records_failed_runs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let result: Result<ImportOutcome, ImportError> =
            Err(ImportError::Gateway(GatewayError::Request));

        record(dir.path(), stamp(0), &result).expect("failed run");

        assert_eq!(
            read(dir.path(), "2026-10-14"),
            "2026-10-14T17:46:40Z error failed to fetch nodes\n"
        );
    }

    #[test]
    fn record_now_uses_the_current_utc_date() {
        let dir = tempfile::tempdir().expect("temp dir");

        record_now(dir.path(), &Ok(ImportOutcome::Imported { count: 1 })).expect("record_now");

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let contents = read(dir.path(), &today);
        assert!(
            contents.starts_with(&format!("{today}T")) && contents.ends_with(" imported count=1\n"),
            "unexpected contents: {contents}"
        );
    }
}
