use crate::domain::Node;
use crate::repositories::mock_nodes_repository::MockNodesRepository;
use crate::repositories::nodes_repository::{NodesRepository, RepositoryError};
use crate::repositories::postgres_nodes_repository::PostgresNodesRepository;
use crate::repositories::sqlite_nodes_repository::SqliteNodesRepository;

/// Runtime-selectable `NodesRepository` wrapper.
///
/// The factory module decides which backend to build from configuration;
/// this enum is the result of that choice. It implements the port by
/// delegating to the selected variant and contains no persistence logic of
/// its own.
pub enum AnyNodesRepository {
    InMemory(MockNodesRepository),
    Postgres(PostgresNodesRepository),
    Sqlite(SqliteNodesRepository),
}

impl NodesRepository for AnyNodesRepository {
    async fn save_snapshot(&self, nodes: &[Node]) -> Result<(), RepositoryError> {
        match self {
            Self::InMemory(repository) => repository.save_snapshot(nodes).await,
            Self::Postgres(repository) => repository.save_snapshot(nodes).await,
            Self::Sqlite(repository) => repository.save_snapshot(nodes).await,
        }
    }

    async fn list(&self) -> Result<Vec<Node>, RepositoryError> {
        match self {
            Self::InMemory(repository) => repository.list().await,
            Self::Postgres(repository) => repository.list().await,
            Self::Sqlite(repository) => repository.list().await,
        }
    }

    async fn ping(&self) -> Result<(), RepositoryError> {
        match self {
            Self::InMemory(repository) => repository.ping().await,
            Self::Postgres(repository) => repository.ping().await,
            Self::Sqlite(repository) => repository.ping().await,
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use crate::domain::{FirstSeen, PublicKey, Sats};

    fn node(key: &str, alias: &str, rank: u32) -> Node {
        Node {
            public_key: PublicKey::parse(key).expect("public key"),
            alias: alias.to_string(),
            capacity: Sats::new(1_000),
            first_seen: FirstSeen::from_unix(1_522_941_222).expect("timestamp"),
            rank_position: rank,
        }
    }

    #[tokio::test]
    async fn forwards_snapshot_roundtrip_to_selected_variant() {
        let repository = AnyNodesRepository::InMemory(MockNodesRepository::new());

        repository
            .save_snapshot(&[node("03aaa", "first", 1)])
            .await
            .expect("save");

        let listed = repository.list().await.expect("list");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].alias, "first");
        repository.ping().await.expect("ping");
    }

    #[tokio::test]
    async fn surfaces_errors_from_selected_variant() {
        let mock = MockNodesRepository::new();
        mock.fail_list();
        let repository = AnyNodesRepository::InMemory(mock);

        assert_eq!(repository.list().await, Err(RepositoryError::Unavailable));
    }
}
