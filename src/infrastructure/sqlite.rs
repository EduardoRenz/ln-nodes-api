use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// Process-scoped SQLite manager.
///
/// Owns the single connection pool and applies migrations. It is created only
/// by the factory that selects SQLite, never by the repository itself.
pub(crate) struct Sqlite {
    pool: SqlitePool,
}

impl Sqlite {
    pub(crate) async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::from_str(database_url)?;
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .after_connect(|connection, _metadata| {
                Box::pin(async move {
                    sqlx::query("PRAGMA journal_mode = WAL")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("PRAGMA foreign_keys = ON")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;

        Ok(Self { pool })
    }

    pub(crate) async fn run_migrations(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations-sqlite").run(&self.pool).await
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
