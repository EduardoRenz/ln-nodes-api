use std::sync::Arc;

use sqlx::FromRow;

use crate::domain::{FirstSeen, Node, PublicKey, Sats};
use crate::infrastructure::sqlite::Sqlite;
use crate::repositories::nodes_repository::{NodesRepository, RepositoryError};

/// SQLite implementation of the `NodesRepository` interface.
///
/// The repository never creates a connection: it receives the process-scoped
/// SQLite manager built by the factory.
pub struct SqliteNodesRepository {
    database: Arc<Sqlite>,
}

impl SqliteNodesRepository {
    pub(crate) fn new(database: Arc<Sqlite>) -> Self {
        Self { database }
    }
}

impl NodesRepository for SqliteNodesRepository {
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
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(node.public_key.as_str())
            .bind(&node.alias)
            .bind(capacity_sats)
            .bind(node.first_seen.as_datetime().timestamp())
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
    first_seen: i64,
    rank_position: i64,
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
            first_seen: FirstSeen::from_unix(row.first_seen)
                .map_err(|_| RepositoryError::InvalidData)?,
            rank_position,
        })
    }
}

fn db_error(error: sqlx::Error) -> RepositoryError {
    tracing::error!(%error, "database operation failed");
    // Keep the cause in the value: callers log it again and the audit line
    // carries it without needing the console.
    RepositoryError::Unavailable(error.to_string())
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn rejects_negative_capacity() {
        let row = NodeRow {
            public_key: "03abc".to_string(),
            alias: "node".to_string(),
            capacity_sats: -1,
            first_seen: 1_522_941_222,
            rank_position: 1,
        };

        assert_eq!(Node::try_from(row), Err(RepositoryError::InvalidData));
    }

    #[test]
    fn rejects_negative_rank_position() {
        let row = NodeRow {
            public_key: "03abc".to_string(),
            alias: "node".to_string(),
            capacity_sats: 1,
            first_seen: 1_522_941_222,
            rank_position: -1,
        };

        assert_eq!(Node::try_from(row), Err(RepositoryError::InvalidData));
    }

    #[test]
    fn rejects_invalid_timestamp() {
        let row = NodeRow {
            public_key: "03abc".to_string(),
            alias: "node".to_string(),
            capacity_sats: 1,
            first_seen: i64::MIN,
            rank_position: 1,
        };

        assert_eq!(Node::try_from(row), Err(RepositoryError::InvalidData));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::repositories::nodes_repository::NodesRepository;

    /// Build a repository backed by a fresh database file in `dir`.
    async fn repository_in(dir: &tempfile::TempDir) -> SqliteNodesRepository {
        let path = dir.path().join("lightning.db");
        let database_url = format!("sqlite://{}?mode=rwc", path.display());

        let database = Sqlite::connect(&database_url)
            .await
            .expect("sqlite connection");

        database.run_migrations().await.expect("sqlite migrations");

        SqliteNodesRepository::new(Arc::new(database))
    }

    fn node(key: &str, alias: &str, capacity: u64, seen: i64, rank: u32) -> Node {
        Node {
            public_key: PublicKey::parse(key).expect("public key"),
            alias: alias.to_string(),
            capacity: Sats::new(capacity),
            first_seen: FirstSeen::from_unix(seen).expect("timestamp"),
            rank_position: rank,
        }
    }

    #[tokio::test]
    async fn saves_and_lists_snapshot_in_rank_order() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repository = repository_in(&dir).await;

        let nodes = vec![
            node("03aaa", "first", 1_000, 1_522_941_222, 2),
            node("03bbb", "second", 2_000, 1_522_941_223, 1),
        ];

        repository.save_snapshot(&nodes).await.expect("save");

        let listed = repository.list().await.expect("list");

        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].public_key.as_str(), "03bbb");
        assert_eq!(listed[1].public_key.as_str(), "03aaa");
        assert_eq!(listed[0], nodes[1]);
        assert_eq!(listed[1], nodes[0]);
    }

    #[tokio::test]
    async fn replaces_previous_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repository = repository_in(&dir).await;

        repository
            .save_snapshot(&[node("03old", "old", 1, 1_522_941_222, 1)])
            .await
            .expect("first save");

        repository
            .save_snapshot(&[
                node("03new-a", "new-a", 10, 1_522_941_223, 1),
                node("03new-b", "new-b", 20, 1_522_941_224, 2),
            ])
            .await
            .expect("second save");

        let listed = repository.list().await.expect("list");

        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].alias, "new-a");
        assert_eq!(listed[1].alias, "new-b");
    }

    #[tokio::test]
    async fn returns_empty_list_for_fresh_database() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repository = repository_in(&dir).await;

        assert!(repository.list().await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn ping_succeeds_on_connected_database() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repository = repository_in(&dir).await;

        repository.ping().await.expect("ping");
    }

    #[tokio::test]
    async fn persists_across_repository_instances() {
        let dir = tempfile::tempdir().expect("temp dir");

        {
            let repository = repository_in(&dir).await;
            repository
                .save_snapshot(&[node("03persist", "persist", 7, 1_522_941_222, 1)])
                .await
                .expect("save");
        }

        let reopened = repository_in(&dir).await;
        let listed = reopened.list().await.expect("list");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].public_key.as_str(), "03persist");
    }
}
