use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_MEMPOOL_URL: &str =
    "https://mempool.space/api/v1/lightning/nodes/rankings/connectivity";
pub const DEFAULT_IMPORT_INTERVAL_SECS: u64 = 300;
pub const MIN_IMPORT_INTERVAL_SECS: u64 = 30;
pub const DEFAULT_RATE_LIMIT_MAX_REQUESTS: u32 = 10;
pub const DEFAULT_RATE_LIMIT_WINDOW_SECS: u64 = 60;
pub const DEFAULT_LOG_DIR: &str = "log";

/// Repository implementation selected by the `PERSISTENCE` variable.
///
/// The composition root maps this value to the matching factory. Values are
/// parsed case-insensitively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryType {
    InMemory,
    Postgres,
    Sqlite,
}

impl RepositoryType {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_uppercase().as_str() {
            "IN_MEMORY" | "IN-MEMORY" => Some(Self::InMemory),
            "POSTGRES" => Some(Self::Postgres),
            "SQLITE" => Some(Self::Sqlite),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub repository_type: RepositoryType,
    pub database_url: String,
    pub port: u16,
    pub import_interval: Duration,
    pub mempool_url: String,
    pub log_filter: String,
    pub rate_limit_max_requests: u32,
    pub rate_limit_window: Duration,
    pub log_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable: {0}")]
    Missing(&'static str),
    #[error("invalid value for {name}: {value}")]
    Invalid { name: &'static str, value: String },
    #[error("IMPORT_INTERVAL_SECS must be at least {min} seconds")]
    IntervalTooSmall { min: u64 },
    #[error("RATE_LIMIT_MAX_REQUESTS must be at least 1 request")]
    ZeroRateLimitMaxRequests,
    #[error("RATE_LIMIT_WINDOW_SECS must be at least 1 second")]
    ZeroRateLimitWindow,
    #[error("DATABASE_URL must start with 'sqlite:' when PERSISTENCE=SQLITE")]
    SqliteDatabaseUrl,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let raw_persistence =
            non_empty(lookup("PERSISTENCE")).ok_or(ConfigError::Missing("PERSISTENCE"))?;
        let repository_type =
            RepositoryType::parse(&raw_persistence).ok_or(ConfigError::Invalid {
                name: "PERSISTENCE",
                value: raw_persistence,
            })?;

        let database_url = match repository_type {
            // The in-memory repository has no database to connect to, so the
            // URL is unused and may be absent.
            RepositoryType::InMemory => non_empty(lookup("DATABASE_URL")).unwrap_or_default(),
            RepositoryType::Postgres => {
                non_empty(lookup("DATABASE_URL")).ok_or(ConfigError::Missing("DATABASE_URL"))?
            }
            RepositoryType::Sqlite => {
                let url = non_empty(lookup("DATABASE_URL"))
                    .ok_or(ConfigError::Missing("DATABASE_URL"))?;

                if !url.starts_with("sqlite:") {
                    return Err(ConfigError::SqliteDatabaseUrl);
                }

                url
            }
        };

        let port = match lookup("PORT") {
            Some(raw) if !raw.trim().is_empty() => {
                raw.parse::<u16>().map_err(|_| ConfigError::Invalid {
                    name: "PORT",
                    value: raw,
                })?
            }
            _ => 3000,
        };

        let interval_secs = match lookup("IMPORT_INTERVAL_SECS") {
            Some(raw) if !raw.trim().is_empty() => {
                raw.parse::<u64>().map_err(|_| ConfigError::Invalid {
                    name: "IMPORT_INTERVAL_SECS",
                    value: raw,
                })?
            }
            _ => DEFAULT_IMPORT_INTERVAL_SECS,
        };

        if interval_secs < MIN_IMPORT_INTERVAL_SECS {
            return Err(ConfigError::IntervalTooSmall {
                min: MIN_IMPORT_INTERVAL_SECS,
            });
        }

        let mempool_url =
            non_empty(lookup("MEMPOOL_API_URL")).unwrap_or_else(|| DEFAULT_MEMPOOL_URL.to_string());
        let log_filter = non_empty(lookup("RUST_LOG")).unwrap_or_else(|| "info".to_string());

        let rate_limit_max_requests = match lookup("RATE_LIMIT_MAX_REQUESTS") {
            Some(raw) if !raw.trim().is_empty() => {
                raw.parse::<u32>().map_err(|_| ConfigError::Invalid {
                    name: "RATE_LIMIT_MAX_REQUESTS",
                    value: raw,
                })?
            }
            _ => DEFAULT_RATE_LIMIT_MAX_REQUESTS,
        };

        if rate_limit_max_requests == 0 {
            return Err(ConfigError::ZeroRateLimitMaxRequests);
        }

        let rate_limit_window_secs = match lookup("RATE_LIMIT_WINDOW_SECS") {
            Some(raw) if !raw.trim().is_empty() => {
                raw.parse::<u64>().map_err(|_| ConfigError::Invalid {
                    name: "RATE_LIMIT_WINDOW_SECS",
                    value: raw,
                })?
            }
            _ => DEFAULT_RATE_LIMIT_WINDOW_SECS,
        };

        if rate_limit_window_secs == 0 {
            return Err(ConfigError::ZeroRateLimitWindow);
        }

        let log_dir = non_empty(lookup("LOG_DIR"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_LOG_DIR));

        Ok(Self {
            repository_type,
            database_url,
            port,
            import_interval: Duration::from_secs(interval_secs),
            mempool_url,
            log_filter,
            rate_limit_max_requests,
            rate_limit_window: Duration::from_secs(rate_limit_window_secs),
            log_dir,
        })
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(values: &[(&str, &str)]) -> Result<Config, ConfigError> {
        Config::from_lookup(|key| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_string())
        })
    }

    fn postgres_config() -> &'static [(&'static str, &'static str)] {
        &[
            ("PERSISTENCE", "POSTGRES"),
            ("DATABASE_URL", "postgres://localhost/lightning"),
        ]
    }

    #[test]
    fn uses_defaults_for_optional_values() {
        let config = config_with(postgres_config()).expect("valid config");

        assert_eq!(config.repository_type, RepositoryType::Postgres);
        assert_eq!(config.port, 3000);
        assert_eq!(config.import_interval, Duration::from_secs(300));
        assert_eq!(config.mempool_url, DEFAULT_MEMPOOL_URL);
        assert_eq!(config.log_filter, "info");
        assert_eq!(
            config.rate_limit_max_requests,
            DEFAULT_RATE_LIMIT_MAX_REQUESTS
        );
        assert_eq!(config.rate_limit_window, Duration::from_secs(60));
        assert_eq!(config.log_dir, PathBuf::from("log"));
    }

    #[test]
    fn overrides_log_dir() {
        let config = config_with(&[
            ("PERSISTENCE", "POSTGRES"),
            ("DATABASE_URL", "postgres://localhost/lightning"),
            ("LOG_DIR", "var/import-logs"),
        ])
        .expect("valid config");

        assert_eq!(config.log_dir, PathBuf::from("var/import-logs"));
    }

    #[test]
    fn parses_rate_limit_settings() {
        let config = config_with(&[
            ("PERSISTENCE", "POSTGRES"),
            ("DATABASE_URL", "postgres://localhost/lightning"),
            ("RATE_LIMIT_MAX_REQUESTS", "5"),
            ("RATE_LIMIT_WINDOW_SECS", "10"),
        ])
        .expect("valid config");

        assert_eq!(config.rate_limit_max_requests, 5);
        assert_eq!(config.rate_limit_window, Duration::from_secs(10));
    }

    #[test]
    fn rejects_zero_rate_limit_max_requests() {
        assert_eq!(
            config_with(&[
                ("PERSISTENCE", "POSTGRES"),
                ("DATABASE_URL", "postgres://localhost/lightning"),
                ("RATE_LIMIT_MAX_REQUESTS", "0"),
            ]),
            Err(ConfigError::ZeroRateLimitMaxRequests)
        );
    }

    #[test]
    fn rejects_zero_rate_limit_window() {
        assert_eq!(
            config_with(&[
                ("PERSISTENCE", "POSTGRES"),
                ("DATABASE_URL", "postgres://localhost/lightning"),
                ("RATE_LIMIT_WINDOW_SECS", "0"),
            ]),
            Err(ConfigError::ZeroRateLimitWindow)
        );
    }

    #[test]
    fn rejects_invalid_rate_limit_max_requests() {
        assert_eq!(
            config_with(&[
                ("PERSISTENCE", "POSTGRES"),
                ("DATABASE_URL", "postgres://localhost/lightning"),
                ("RATE_LIMIT_MAX_REQUESTS", "many"),
            ]),
            Err(ConfigError::Invalid {
                name: "RATE_LIMIT_MAX_REQUESTS",
                value: "many".to_string(),
            })
        );
    }

    #[test]
    fn rejects_missing_persistence() {
        assert_eq!(config_with(&[]), Err(ConfigError::Missing("PERSISTENCE")));
    }

    #[test]
    fn rejects_unknown_persistence() {
        assert_eq!(
            config_with(&[("PERSISTENCE", "ROCKSDB")]),
            Err(ConfigError::Invalid {
                name: "PERSISTENCE",
                value: "ROCKSDB".to_string(),
            })
        );
    }

    #[test]
    fn parses_persistence_case_insensitively() {
        assert_eq!(
            config_with(&[
                ("PERSISTENCE", "postgres"),
                ("DATABASE_URL", "postgres://localhost/lightning"),
            ])
            .map(|config| config.repository_type),
            Ok(RepositoryType::Postgres)
        );
        assert_eq!(
            config_with(&[("PERSISTENCE", "in_memory")]).map(|config| config.repository_type),
            Ok(RepositoryType::InMemory)
        );
        assert_eq!(
            config_with(&[
                ("PERSISTENCE", "Sqlite"),
                ("DATABASE_URL", "sqlite://lightning.db"),
            ])
            .map(|config| config.repository_type),
            Ok(RepositoryType::Sqlite)
        );
    }

    #[test]
    fn rejects_missing_database_url_for_postgres() {
        assert_eq!(
            config_with(&[("PERSISTENCE", "POSTGRES")]),
            Err(ConfigError::Missing("DATABASE_URL"))
        );
    }

    #[test]
    fn rejects_missing_database_url_for_sqlite() {
        assert_eq!(
            config_with(&[("PERSISTENCE", "SQLITE")]),
            Err(ConfigError::Missing("DATABASE_URL"))
        );
    }

    #[test]
    fn rejects_non_sqlite_url_for_sqlite() {
        assert_eq!(
            config_with(&[
                ("PERSISTENCE", "SQLITE"),
                ("DATABASE_URL", "postgres://localhost/lightning"),
            ]),
            Err(ConfigError::SqliteDatabaseUrl)
        );
    }

    #[test]
    fn accepts_sqlite_url_for_sqlite() {
        let config = config_with(&[
            ("PERSISTENCE", "SQLITE"),
            ("DATABASE_URL", "sqlite://lightning.db?mode=rwc"),
        ])
        .expect("valid config");

        assert_eq!(config.database_url, "sqlite://lightning.db?mode=rwc");
    }

    #[test]
    fn in_memory_needs_no_database_url() {
        let config = config_with(&[("PERSISTENCE", "IN_MEMORY")]).expect("valid config");

        assert_eq!(config.repository_type, RepositoryType::InMemory);
        assert_eq!(config.database_url, "");
    }

    #[test]
    fn rejects_too_small_import_interval() {
        assert_eq!(
            config_with(&[
                ("PERSISTENCE", "POSTGRES"),
                ("DATABASE_URL", "postgres://localhost/lightning"),
                ("IMPORT_INTERVAL_SECS", "5"),
            ]),
            Err(ConfigError::IntervalTooSmall { min: 30 })
        );
    }
}
