use std::sync::{Arc, Mutex};

use crate::domain::Node;
use crate::gateways::nodes_gateway::{GatewayError, NodesGateway};

#[derive(Debug, Clone)]
pub struct MockNodesGateway {
    state: Arc<Mutex<MockState>>,
}

#[derive(Debug)]
struct MockState {
    result: Result<Vec<Node>, GatewayError>,
    fetch_calls: usize,
}

impl Default for MockState {
    fn default() -> Self {
        Self {
            result: Ok(Vec::new()),
            fetch_calls: 0,
        }
    }
}

impl Default for MockNodesGateway {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState::default())),
        }
    }
}

impl MockNodesGateway {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_nodes(nodes: Vec<Node>) -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                result: Ok(nodes),
                fetch_calls: 0,
            })),
        }
    }

    pub fn failing(error: GatewayError) -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                result: Err(error),
                fetch_calls: 0,
            })),
        }
    }

    pub fn fetch_calls(&self) -> usize {
        self.state().fetch_calls
    }

    fn state(&self) -> std::sync::MutexGuard<'_, MockState> {
        self.state.lock().expect("mock gateway mutex poisoned")
    }
}

impl NodesGateway for MockNodesGateway {
    async fn fetch(&self) -> Result<Vec<Node>, GatewayError> {
        let mut state = self.state();
        state.fetch_calls += 1;
        state.result.clone()
    }
}
