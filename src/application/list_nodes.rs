use crate::domain::Node;
use crate::repositories::nodes_repository::{NodesRepository, RepositoryError};

#[derive(Debug, thiserror::Error)]
pub enum ListError {
    // Source embedded in the display: the HTTP layer logs this value, so the
    // concrete database cause reaches the console and the access trail.
    #[error("failed to list nodes: {0}")]
    Repository(#[source] RepositoryError),
}

pub async fn list_nodes<R>(repository: &R) -> Result<Vec<Node>, ListError>
where
    R: NodesRepository,
{
    repository.list().await.map_err(ListError::Repository)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FirstSeen, PublicKey, Sats};
    use crate::repositories::mock_nodes_repository::MockNodesRepository;

    fn node() -> Node {
        Node {
            public_key: PublicKey::parse("node-1").expect("valid public key"),
            alias: "test".to_string(),
            capacity: Sats::new(1_000),
            first_seen: FirstSeen::from_unix(1_522_941_222).expect("valid timestamp"),
            rank_position: 0,
        }
    }

    #[tokio::test]
    async fn returns_repository_nodes() {
        let repository = MockNodesRepository::with_nodes(vec![node()]);

        let nodes = list_nodes(&repository).await.expect("list");

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].public_key.as_str(), "node-1");
    }

    #[tokio::test]
    async fn maps_repository_errors() {
        let repository = MockNodesRepository::new();
        repository.fail_list();

        let result = list_nodes(&repository).await;

        assert!(matches!(
            result,
            Err(ListError::Repository(RepositoryError::Unavailable(_)))
        ));
    }

    #[test]
    fn error_display_carries_the_database_cause() {
        let error =
            ListError::Repository(RepositoryError::Unavailable("table missing".to_string()));

        assert_eq!(error.to_string(), "failed to list nodes: table missing");
    }
}
