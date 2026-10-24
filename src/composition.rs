use std::sync::Arc;
use std::time::Duration;

use axum::Router;

use crate::config::{Config, RepositoryType};
use crate::gateways::nodes_gateway::NodesGateway;
use crate::infrastructure::postgres::Postgres;
use crate::infrastructure::sqlite::Sqlite;
use crate::repositories::nodes_repository::NodesRepository;
use crate::repositories::nodes_repository_factory::RepositoryFactoryError;
use crate::routes;
use crate::routes::RateLimiter;

/// The wired application: HTTP router, gateway, repository, and import cadence.
pub struct App<G: NodesGateway, R: NodesRepository> {
    pub router: Router,
    pub gateway: Arc<G>,
    pub repository: Arc<R>,
    pub import_interval: Duration,
}

/// Wires already constructed dependencies into the application graph.
///
/// Dependencies are created by their factories in the composition root and
/// injected here, so this function stays free of infrastructure knowledge.
pub fn build_app<G, R>(gateway: G, repository: R, config: &Config) -> App<G, R>
where
    G: NodesGateway + 'static,
    R: NodesRepository + 'static,
{
    let gateway = Arc::new(gateway);
    let repository = Arc::new(repository);
    let limiter = RateLimiter::new(config.rate_limit_max_requests, config.rate_limit_window);
    let router = routes::router(repository.clone(), limiter);

    App {
        router,
        gateway,
        repository,
        import_interval: config.import_interval,
    }
}

/// Applies pending database migrations for the configured backend.
///
/// Used by the `migrate` command. Migrations are a database lifecycle
/// operation, so they live outside the repository factory. The in-memory
/// repository has nothing to migrate.
pub async fn run_migrations(config: &Config) -> Result<(), RepositoryFactoryError> {
    match config.repository_type {
        RepositoryType::Postgres => {
            let database = Postgres::connect(&config.database_url)
                .await
                .map_err(RepositoryFactoryError::Database)?;

            database
                .run_migrations()
                .await
                .map_err(RepositoryFactoryError::Migration)
        }
        RepositoryType::Sqlite => {
            let database = Sqlite::connect(&config.database_url)
                .await
                .map_err(RepositoryFactoryError::Database)?;

            database
                .run_migrations()
                .await
                .map_err(RepositoryFactoryError::Migration)
        }
        RepositoryType::InMemory => {
            tracing::info!("in-memory repository selected; nothing to migrate");
            Ok(())
        }
    }
}
