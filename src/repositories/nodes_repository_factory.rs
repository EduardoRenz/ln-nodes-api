use std::future::Future;
use std::sync::Arc;

use crate::config::{Config, RepositoryType};
use crate::infrastructure::postgres::Postgres;
use crate::infrastructure::sqlite::Sqlite;
use crate::repositories::any_nodes_repository::AnyNodesRepository;
use crate::repositories::mock_nodes_repository::MockNodesRepository;
use crate::repositories::nodes_repository::NodesRepository;
use crate::repositories::postgres_nodes_repository::PostgresNodesRepository;
use crate::repositories::sqlite_nodes_repository::SqliteNodesRepository;

#[derive(Debug, thiserror::Error)]
pub enum RepositoryFactoryError {
    // Both embed the sqlx cause so a failed startup or CLI import explains
    // itself on stderr and in the audit record.
    #[error("failed to connect to PostgreSQL: {0}")]
    Database(#[source] sqlx::Error),
    #[error("failed to run database migrations: {0}")]
    Migration(#[source] sqlx::migrate::MigrateError),
}

/// Creates `NodesRepository` implementations.
///
/// A factory only builds and returns the implementation. It never implements
/// the repository behavior itself, and it is never a global accessor.
pub trait NodesRepositoryFactory: Send + Sync {
    type Repository: NodesRepository + 'static;

    fn create(
        &self,
        config: &Config,
    ) -> impl Future<Output = Result<Self::Repository, RepositoryFactoryError>> + Send;
}

pub struct PostgresNodesRepositoryFactory;

impl NodesRepositoryFactory for PostgresNodesRepositoryFactory {
    type Repository = PostgresNodesRepository;

    async fn create(&self, config: &Config) -> Result<Self::Repository, RepositoryFactoryError> {
        let database = Postgres::connect(&config.database_url)
            .await
            .map_err(RepositoryFactoryError::Database)?;

        database
            .run_migrations()
            .await
            .map_err(RepositoryFactoryError::Migration)?;

        Ok(PostgresNodesRepository::new(Arc::new(database)))
    }
}

pub struct SqliteNodesRepositoryFactory;

impl NodesRepositoryFactory for SqliteNodesRepositoryFactory {
    type Repository = SqliteNodesRepository;

    async fn create(&self, config: &Config) -> Result<Self::Repository, RepositoryFactoryError> {
        let database = Sqlite::connect(&config.database_url)
            .await
            .map_err(RepositoryFactoryError::Database)?;

        database
            .run_migrations()
            .await
            .map_err(RepositoryFactoryError::Migration)?;

        Ok(SqliteNodesRepository::new(Arc::new(database)))
    }
}

pub struct MockNodesRepositoryFactory;

impl NodesRepositoryFactory for MockNodesRepositoryFactory {
    type Repository = MockNodesRepository;

    async fn create(&self, _config: &Config) -> Result<Self::Repository, RepositoryFactoryError> {
        Ok(MockNodesRepository::new())
    }
}

/// Builds the repository variant selected by `PERSISTENCE`.
///
/// Selection happens exactly once, in the composition root. This factory only
/// delegates to the per-backend factory; it never implements repository
/// behavior itself.
pub struct ConfiguredNodesRepositoryFactory;

impl NodesRepositoryFactory for ConfiguredNodesRepositoryFactory {
    type Repository = AnyNodesRepository;

    async fn create(&self, config: &Config) -> Result<Self::Repository, RepositoryFactoryError> {
        match config.repository_type {
            RepositoryType::InMemory => MockNodesRepositoryFactory
                .create(config)
                .await
                .map(AnyNodesRepository::InMemory),
            RepositoryType::Postgres => PostgresNodesRepositoryFactory
                .create(config)
                .await
                .map(AnyNodesRepository::Postgres),
            RepositoryType::Sqlite => SqliteNodesRepositoryFactory
                .create(config)
                .await
                .map(AnyNodesRepository::Sqlite),
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn in_memory_config() -> Config {
        Config {
            repository_type: RepositoryType::InMemory,
            database_url: String::new(),
            port: 3000,
            import_interval: std::time::Duration::from_secs(300),
            mempool_url: "http://localhost".to_string(),
            log_filter: "info".to_string(),
            rate_limit_max_requests: 60,
            rate_limit_window: std::time::Duration::from_secs(60),
            log_backend: crate::config::LogBackend::File,
            log_dir: std::path::PathBuf::from("log"),
        }
    }

    #[tokio::test]
    async fn selects_in_memory_variant_from_config() {
        let repository = ConfiguredNodesRepositoryFactory
            .create(&in_memory_config())
            .await
            .expect("factory");

        assert!(matches!(repository, AnyNodesRepository::InMemory(_)));
    }

    #[test]
    fn factory_error_display_carries_the_sqlx_cause() {
        let error = RepositoryFactoryError::Database(sqlx::Error::PoolTimedOut);

        let rendered = error.to_string();
        assert!(
            rendered.starts_with("failed to connect to PostgreSQL: "),
            "unexpected message: {rendered}"
        );
        assert!(rendered.contains("pool timed out"));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::domain::{FirstSeen, Node, PublicKey, Sats};

    #[tokio::test]
    async fn selects_sqlite_variant_and_persists_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("lightning.db");

        let config = Config {
            repository_type: RepositoryType::Sqlite,
            database_url: format!("sqlite://{}?mode=rwc", path.display()),
            port: 3000,
            import_interval: std::time::Duration::from_secs(300),
            mempool_url: "http://localhost".to_string(),
            log_filter: "info".to_string(),
            rate_limit_max_requests: 60,
            rate_limit_window: std::time::Duration::from_secs(60),
            log_backend: crate::config::LogBackend::File,
            log_dir: std::path::PathBuf::from("log"),
        };

        let repository = ConfiguredNodesRepositoryFactory
            .create(&config)
            .await
            .expect("factory");

        assert!(matches!(repository, AnyNodesRepository::Sqlite(_)));

        repository
            .save_snapshot(&[Node {
                public_key: PublicKey::parse("03persist").expect("public key"),
                alias: "persist".to_string(),
                capacity: Sats::new(7),
                first_seen: FirstSeen::from_unix(1_522_941_222).expect("timestamp"),
                rank_position: 1,
            }])
            .await
            .expect("save");

        repository.ping().await.expect("ping");

        let listed = repository.list().await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].public_key.as_str(), "03persist");
    }
}
