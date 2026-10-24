use crate::domain::Node;
use crate::gateways::nodes_gateway::{GatewayError, NodesGateway};
use crate::repositories::nodes_repository::{NodesRepository, RepositoryError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    SkippedEmpty,
    /// The snapshot was replaced: `count` is its total size, `new` is how
    /// many public keys were absent from the previous snapshot.
    Imported {
        count: usize,
        new: usize,
    },
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
    // The source is embedded in the display so every consumer — console,
    // audit line, exit output — carries the cause without walking the chain.
    #[error("failed to fetch nodes: {0}")]
    Gateway(#[source] GatewayError),
    // Covers both the baseline read (list) and the write (save_snapshot):
    // either way the repository could not serve the run.
    #[error("repository operation failed: {0}")]
    Repository(#[source] RepositoryError),
}

/// Counts how many `incoming` public keys are absent from `previous`.
///
/// Pure helper so the delta rule is testable without a repository.
pub fn count_new(previous: &[Node], incoming: &[Node]) -> usize {
    incoming
        .iter()
        .filter(|node| !previous.iter().any(|old| old.public_key == node.public_key))
        .count()
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
            // Read the current snapshot before replacing it so the outcome
            // can report how many rows are genuinely new. A read failure
            // aborts the run like a write failure would: with no baseline the
            // delta would be a lie, and an unreachable repository already
            // means the snapshot must be kept.
            let previous = repository.list().await.map_err(ImportError::Repository)?;
            let new = count_new(&previous, &nodes);

            repository
                .save_snapshot(&nodes)
                .await
                .map_err(ImportError::Repository)?;
            Ok(ImportOutcome::Imported {
                count: nodes.len(),
                new,
            })
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

        assert_eq!(outcome, ImportOutcome::Imported { count: 1, new: 1 });
        assert_eq!(repository.snapshot().len(), 1);
        assert_eq!(repository.save_calls(), 1);
    }

    #[tokio::test]
    async fn counts_only_rows_absent_from_the_previous_snapshot() {
        // Previous snapshot holds node-1 and node-2; the gateway drops node-1
        // and brings node-3: total = 2 (node-2, node-3), new = 1 (node-3).
        let repository = MockNodesRepository::with_nodes(vec![node("node-1"), node("node-2")]);
        let gateway = MockNodesGateway::with_nodes(vec![node("node-2"), node("node-3")]);

        let outcome = import_nodes(&gateway, &repository).await.expect("import");

        assert_eq!(outcome, ImportOutcome::Imported { count: 2, new: 1 });
        assert_eq!(
            repository
                .snapshot()
                .iter()
                .map(|n| n.public_key.as_str())
                .collect::<Vec<_>>(),
            vec!["node-2", "node-3"],
        );
    }

    #[tokio::test]
    async fn reports_zero_new_rows_when_the_snapshot_is_unchanged() {
        let repository = MockNodesRepository::with_nodes(vec![node("node-1")]);
        let gateway = MockNodesGateway::with_nodes(vec![node("node-1")]);

        let outcome = import_nodes(&gateway, &repository).await.expect("import");

        assert_eq!(outcome, ImportOutcome::Imported { count: 1, new: 0 });
    }

    #[tokio::test]
    async fn does_not_persist_when_the_baseline_cannot_be_read() {
        let gateway = MockNodesGateway::with_nodes(vec![node("node-1")]);
        let repository = MockNodesRepository::new();
        repository.fail_list();

        let result = import_nodes(&gateway, &repository).await;

        assert!(matches!(result, Err(ImportError::Repository(_))));
        assert_eq!(repository.save_calls(), 0);
    }

    #[test]
    fn count_new_ignores_shared_and_dropped_keys() {
        let previous = vec![node("a"), node("b")];
        let incoming = vec![node("b"), node("c"), node("d")];

        // "b" already existed; "c" and "d" are new.
        assert_eq!(count_new(&previous, &incoming), 2);
        assert_eq!(count_new(&[], &incoming), 3);
        assert_eq!(count_new(&previous, &[]), 0);
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

    #[test]
    fn error_displays_carry_the_cause() {
        let repository = ImportError::Repository(RepositoryError::Unavailable(
            "connection refused".to_string(),
        ));
        assert_eq!(
            repository.to_string(),
            "repository operation failed: connection refused"
        );

        let gateway = ImportError::Gateway(GatewayError::Request);
        assert_eq!(
            gateway.to_string(),
            "failed to fetch nodes: mempool request failed"
        );
    }
}
