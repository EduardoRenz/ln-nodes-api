use crate::domain::Node;
use crate::gateways::nodes_gateway::{GatewayError, NodesGateway};
use crate::repositories::nodes_repository::{NodesRepository, RepositoryError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    SkippedEmpty,
    Imported { count: usize },
}

pub enum ImportPlan {
    Skip,
    Replace(Vec<Node>),
}

pub fn plan_import(nodes: Vec<Node>) -> ImportPlan {
    if nodes.is_empty() {
        ImportPlan::Skip
    } else {
        ImportPlan::Replace(nodes)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("failed to fetch nodes")]
    Gateway(#[source] GatewayError),
    #[error("failed to persist nodes")]
    Repository(#[source] RepositoryError),
}

pub async fn import_nodes<G, R>(gateway: &G, repository: &R) -> Result<ImportOutcome, ImportError>
where
    G: NodesGateway,
    R: NodesRepository,
{
    let nodes = gateway.fetch().await.map_err(ImportError::Gateway)?;

    match plan_import(nodes) {
        ImportPlan::Skip => Ok(ImportOutcome::SkippedEmpty),
        ImportPlan::Replace(nodes) => {
            repository
                .save_snapshot(&nodes)
                .await
                .map_err(ImportError::Repository)?;
            Ok(ImportOutcome::Imported { count: nodes.len() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FirstSeen, PublicKey, Sats};
    use crate::gateways::mock_nodes_gateway::MockNodesGateway;
    use crate::repositories::mock_nodes_repository::MockNodesRepository;

    fn node(public_key: &str) -> Node {
        Node {
            public_key: PublicKey::parse(public_key).expect("valid public key"),
            alias: "test".to_string(),
            capacity: Sats::new(1_000),
            first_seen: FirstSeen::from_unix(1_522_941_222).expect("valid timestamp"),
            rank_position: 0,
        }
    }

    #[tokio::test]
    async fn skips_empty_snapshots() {
        let gateway = MockNodesGateway::new();
        let repository = MockNodesRepository::new();

        let outcome = import_nodes(&gateway, &repository).await.expect("import");

        assert_eq!(outcome, ImportOutcome::SkippedEmpty);
        assert_eq!(repository.save_calls(), 0);
    }

    #[tokio::test]
    async fn persists_non_empty_snapshots() {
        let gateway = MockNodesGateway::with_nodes(vec![node("node-1")]);
        let repository = MockNodesRepository::new();

        let outcome = import_nodes(&gateway, &repository).await.expect("import");

        assert_eq!(outcome, ImportOutcome::Imported { count: 1 });
        assert_eq!(repository.snapshot().len(), 1);
        assert_eq!(repository.save_calls(), 1);
    }

    #[tokio::test]
    async fn does_not_persist_when_the_gateway_fails() {
        let gateway = MockNodesGateway::failing(GatewayError::Request);
        let repository = MockNodesRepository::new();

        let result = import_nodes(&gateway, &repository).await;

        assert!(matches!(result, Err(ImportError::Gateway(_))));
        assert_eq!(repository.save_calls(), 0);
    }

    #[tokio::test]
    async fn propagates_repository_failures() {
        let gateway = MockNodesGateway::with_nodes(vec![node("node-1")]);
        let repository = MockNodesRepository::new();
        repository.fail_save();

        let result = import_nodes(&gateway, &repository).await;

        assert!(matches!(result, Err(ImportError::Repository(_))));
    }
}
