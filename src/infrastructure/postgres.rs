use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Process-scoped PostgreSQL manager.
///
/// Owns the single connection pool and applies migrations. It is created only
/// by the factory that selects PostgreSQL, never by the repository itself.
pub(crate) struct Postgres {
    pool: PgPool,
}

impl Postgres {
    pub(crate) async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = connect_with_retry(database_url).await?;
        Ok(Self { pool })
    }

    pub(crate) async fn run_migrations(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}

async fn connect_with_retry(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let mut delay = Duration::from_millis(500);
    let mut attempt = 0;

    loop {
        attempt += 1;

        match make_pool(database_url).await {
            Ok(pool) => return Ok(pool),
            Err(error) if attempt >= 10 => {
                tracing::error!(attempt, %error, "database connection attempts exhausted");
                return Err(error);
            }
            Err(error) => {
                tracing::warn!(attempt, %error, "database connection failed; retrying");
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(5));
            }
        }
    }
}

async fn make_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout = '5s'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await
}
