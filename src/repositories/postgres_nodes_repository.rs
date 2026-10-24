use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::FromRow;

use crate::domain::{FirstSeen, Node, PublicKey, Sats};
use crate::infrastructure::postgres::Postgres;
use crate::repositories::nodes_repository::{NodesRepository, RepositoryError};

/// PostgreSQL implementation of the `NodesRepository` interface.
///
/// The repository never creates a connection: it receives the process-scoped
/// PostgreSQL manager built by the factory.
pub struct PostgresNodesRepository {
    database: Arc<Postgres>,
}

impl PostgresNodesRepository {
    pub(crate) fn new(database: Arc<Postgres>) -> Self {
        Self { database }
    }
}

impl NodesRepository for PostgresNodesRepository {
    async fn save_snapshot(&self, nodes: &[Node]) -> Result<(), RepositoryError> {
        let mut transaction = self.database.pool().begin().await.map_err(db_error)?;

        sqlx::query("DELETE FROM nodes")
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;

        for node in nodes {
            let capacity_sats =
                i64::try_from(node.capacity.as_u64()).map_err(|_| RepositoryError::InvalidData)?;

            sqlx::query(
                "INSERT INTO nodes \
                 (public_key, alias, capacity_sats, first_seen, rank_position) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(node.public_key.as_str())
            .bind(&node.alias)
            .bind(capacity_sats)
            .bind(node.first_seen.as_datetime())
            .bind(i64::from(node.rank_position))
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        }

        transaction.commit().await.map_err(db_error)
    }

    async fn list(&self) -> Result<Vec<Node>, RepositoryError> {
        let rows = sqlx::query_as::<_, NodeRow>(
            "SELECT public_key, alias, capacity_sats, first_seen, rank_position \
             FROM nodes ORDER BY rank_position ASC",
        )
        .fetch_all(self.database.pool())
        .await
        .map_err(db_error)?;

        rows.into_iter().map(Node::try_from).collect()
    }

    async fn ping(&self) -> Result<(), RepositoryError> {
        sqlx::query("SELECT 1")
            .execute(self.database.pool())
            .await
            .map(|_| ())
            .map_err(db_error)
    }
}

#[derive(Debug, FromRow)]
struct NodeRow {
    public_key: String,
    alias: String,
    capacity_sats: i64,
    first_seen: DateTime<Utc>,
    rank_position: i32,
}

impl TryFrom<NodeRow> for Node {
    type Error = RepositoryError;

    fn try_from(row: NodeRow) -> Result<Self, Self::Error> {
        let capacity =
            u64::try_from(row.capacity_sats).map_err(|_| RepositoryError::InvalidData)?;
        let rank_position =
            u32::try_from(row.rank_position).map_err(|_| RepositoryError::InvalidData)?;

        Ok(Self {
            public_key: PublicKey::parse(row.public_key)
                .map_err(|_| RepositoryError::InvalidData)?,
            alias: row.alias,
            capacity: Sats::new(capacity),
            first_seen: FirstSeen::from_unix(row.first_seen.timestamp())
                .map_err(|_| RepositoryError::InvalidData)?,
            rank_position,
        })
    }
}

fn db_error(error: sqlx::Error) -> RepositoryError {
    tracing::error!(%error, "database operation failed");
    // Keep the cause in the value: callers log it again, the HTTP layer
    // embeds it in `request failed`, and the audit line carries it too.
    RepositoryError::Unavailable(error.to_string())
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn db_error_carries_the_sqlx_cause() {
        let error = db_error(sqlx::Error::PoolTimedOut);

        // The value IS the cause; the `database operation failed` context
        // lives in the boundary log line above.
        let rendered = error.to_string();
        assert!(
            rendered.contains("pool timed out"),
            "unexpected message: {rendered}"
        );
    }
}
