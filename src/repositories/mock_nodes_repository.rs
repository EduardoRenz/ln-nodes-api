use std::sync::{Arc, Mutex};

use crate::domain::Node;
use crate::repositories::nodes_repository::{NodesRepository, RepositoryError};

/// In-memory implementation of the `NodesRepository` interface.
///
/// The mock doubles as the runtime `IN_MEMORY` backend: it is compiled
/// outside test builds so the composition root can select it through
/// configuration. The `fail_*` helpers exist only for tests; production
/// code never calls them.
#[derive(Debug, Clone, Default)]
pub struct MockNodesRepository {
    state: Arc<Mutex<MockState>>,
}

#[derive(Debug, Default)]
struct MockState {
    nodes: Vec<Node>,
    save_calls: usize,
    fail_save: bool,
    fail_list: bool,
    fail_ping: bool,
}

impl MockNodesRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_nodes(nodes: Vec<Node>) -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                nodes,
                ..MockState::default()
            })),
        }
    }

    pub fn fail_save(&self) {
        self.state().fail_save = true;
    }

    pub fn fail_list(&self) {
        self.state().fail_list = true;
    }

    pub fn fail_ping(&self) {
        self.state().fail_ping = true;
    }

    pub fn snapshot(&self) -> Vec<Node> {
        self.state().nodes.clone()
    }

    pub fn save_calls(&self) -> usize {
        self.state().save_calls
    }

    fn state(&self) -> std::sync::MutexGuard<'_, MockState> {
        self.state.lock().expect("mock repository mutex poisoned")
    }
}

impl NodesRepository for MockNodesRepository {
    async fn save_snapshot(&self, nodes: &[Node]) -> Result<(), RepositoryError> {
        let mut state = self.state();

        if state.fail_save {
            return Err(RepositoryError::Unavailable);
        }

        state.save_calls += 1;
        state.nodes = nodes.to_vec();
        Ok(())
    }

    async fn list(&self) -> Result<Vec<Node>, RepositoryError> {
        let state = self.state();

        if state.fail_list {
            return Err(RepositoryError::Unavailable);
        }

        Ok(state.nodes.clone())
    }

    async fn ping(&self) -> Result<(), RepositoryError> {
        let state = self.state();

        if state.fail_ping {
            return Err(RepositoryError::Unavailable);
        }

        Ok(())
    }
}
