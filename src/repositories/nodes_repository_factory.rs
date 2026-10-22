use std::future::Future;
use std::sync::Arc;

use crate::config::Config;
use crate::infrastructure::postgres::Postgres;
use crate::repositories::nodes_repository::NodesRepository;
use crate::repositories::postgres_nodes_repository::PostgresNodesRepository;

#[derive(Debug, thiserror::Error)]
pub enum RepositoryFactoryError {
    #[error("failed to connect to PostgreSQL")]
    Database(#[source] sqlx::Error),
    #[error("failed to run database migrations")]
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

// Future: a fully mocked environment selected through configuration.
//
// The mock repository must be compiled outside test builds, for example behind
// a feature flag, and the composition root must pick the factory at runtime.
//
// pub struct MockNodesRepositoryFactory;
//
// impl NodesRepositoryFactory for MockNodesRepositoryFactory {
//     type Repository = MockNodesRepository;
//
//     async fn create(
//         &self,
//         _config: &Config,
//     ) -> Result<Self::Repository, RepositoryFactoryError> {
//         Ok(MockNodesRepository::new())
//     }
// }
