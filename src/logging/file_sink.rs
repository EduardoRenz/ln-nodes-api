//! Date-partitioned append-only file writer.
//!
//! [`FileSink`] is the reusable file mechanics behind the import-run log:
//! it turns an origin name and a filename pattern into a path, creates the
//! directory on demand, and appends one line at a time. It is a plain value
//! with no global state, so any module may build one for its own diagnostics
//! by choice:
//!
//! ```ignore
//! let sink = FileSink::new("nodes_gateway");
//! sink.append("fetch failed: connection refused")?;
//! // → log/2026-10-24_nodes_gateway.log
//! ```
//!
//! Nothing in the service wires such a sink up yet; the import-run backend is
//! the only in-tree consumer.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::config::DEFAULT_LOG_DIR;

/// A write failure carrying the path it tried to touch, so callers can
/// report a self-contained message.
#[derive(Debug, thiserror::Error)]
#[error("failed to write log at {path}")]
pub struct SinkError {
    pub path: PathBuf,
    #[source]
    pub source: io::Error,
}

/// Appends lines to files named by a date/origin pattern.
///
/// Concrete by design: there is one implementation that earns its keep (the
/// import-run backend), so the type stays a simple struct instead of an
/// inner trait. Opening the file per append keeps the sink stateless —
/// creating many instances never leaks descriptors — and `O_APPEND` keeps
/// concurrent writers from interleaving lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSink {
    dir: PathBuf,
    origin: String,
    pattern: String,
}

impl FileSink {
    /// Sink writing under [`DEFAULT_LOG_DIR`] with the default
    /// `{date}_{origin}.log` pattern.
    pub fn new(origin: impl Into<String>) -> Self {
        Self {
            dir: PathBuf::from(DEFAULT_LOG_DIR),
            origin: origin.into(),
            pattern: "{date}_{origin}.log".to_string(),
        }
    }

    /// Overrides the directory the files are created in.
    pub fn in_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = dir.into();
        self
    }

    /// Overrides the filename pattern. Supported placeholders: `{date}`
    /// (`YYYY-MM-DD`, UTC) and `{origin}`. A pattern without `{date}`
    /// appends to a single ever-growing file.
    pub fn pattern(mut self, pattern: impl Into<String>) -> Self {
        self.pattern = pattern.into();
        self
    }

    /// The file `line` written at `at` would go to.
    pub fn path_for(&self, at: DateTime<Utc>) -> PathBuf {
        let name = self
            .pattern
            .replace("{date}", &at.format("%Y-%m-%d").to_string())
            .replace("{origin}", &self.origin);
        self.dir.join(name)
    }

    /// Appends `line` (plus newline) to the file for the current UTC date,
    /// creating the directory when missing.
    pub fn append(&self, line: &str) -> Result<(), SinkError> {
        self.append_at(Utc::now(), line)
    }

    /// [`Self::append`] with an explicit timestamp — the entry point tests
    /// use to pin the date partition.
    pub fn append_at(&self, at: DateTime<Utc>, line: &str) -> Result<(), SinkError> {
        fs::create_dir_all(&self.dir).map_err(|source| SinkError {
            path: self.dir.clone(),
            source,
        })?;

        let path = self.path_for(at);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| SinkError {
                path: path.clone(),
                source,
            })?;

        writeln!(file, "{line}").map_err(|source| SinkError { path, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn stamp(offset_secs: i64) -> DateTime<Utc> {
        // 2026-10-14T17:46:40Z, plus one day per test variant.
        Utc.timestamp_opt(1_792_000_000 + offset_secs, 0)
            .single()
            .expect("timestamp")
    }

    #[test]
    fn default_pattern_embeds_date_and_origin() {
        let sink = FileSink::new("nodes_gateway").in_dir("/var/log/app");

        assert_eq!(
            sink.path_for(stamp(0)),
            PathBuf::from("/var/log/app/2026-10-14_nodes_gateway.log")
        );
    }

    #[test]
    fn custom_pattern_and_dir_are_honoured() {
        let sink = FileSink::new("gateway")
            .in_dir("logs")
            .pattern("{origin}.{date}.txt");

        assert_eq!(
            sink.path_for(stamp(86_400)),
            PathBuf::from("logs/gateway.2026-10-15.txt")
        );
    }

    #[test]
    fn creates_missing_directory_and_appends_lines() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("deep").join("logs");
        let sink = FileSink::new("probe").in_dir(&nested);

        sink.append_at(stamp(0), "first").expect("first append");
        sink.append_at(stamp(0), "second").expect("second append");

        let path = nested.join("2026-10-14_probe.log");
        assert_eq!(fs::read_to_string(&path).expect("file"), "first\nsecond\n");
    }

    #[test]
    fn reports_the_path_when_the_directory_is_unreachable() {
        // A file where a directory should be: create_dir_all must fail and
        // the error must name the directory it tried to create.
        let dir = tempfile::tempdir().expect("temp dir");
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "not a directory").expect("blocker file");

        let sink = FileSink::new("probe").in_dir(&blocker);
        let error = sink.append("line").expect_err("must fail");

        assert_eq!(error.path, blocker);
        assert!(error.to_string().contains("failed to write log at"));
    }
}
