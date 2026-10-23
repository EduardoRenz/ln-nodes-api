use crate::config::Config;
use crate::gateways::mempool_nodes_gateway::MempoolNodesGateway;
use crate::gateways::nodes_gateway::NodesGateway;

#[derive(Debug, thiserror::Error)]
pub enum GatewayFactoryError {
    #[error("failed to build the mempool gateway")]
    Client(#[source] reqwest::Error),
}

/// Creates `NodesGateway` implementations.
///
/// The factory only builds and returns the implementation. It never implements
/// the gateway behavior itself, and it is never a global accessor.
pub trait NodesGatewayFactory: Send + Sync {
    type Gateway: NodesGateway + 'static;

    fn create(&self, config: &Config) -> Result<Self::Gateway, GatewayFactoryError>;
}

pub struct MempoolNodesGatewayFactory;

impl NodesGatewayFactory for MempoolNodesGatewayFactory {
    type Gateway = MempoolNodesGateway;

    fn create(&self, config: &Config) -> Result<Self::Gateway, GatewayFactoryError> {
        MempoolNodesGateway::new(&config.mempool_url).map_err(GatewayFactoryError::Client)
    }
}

// Future: a fully mocked environment selected through configuration.
//
// The mock gateway must be compiled outside test builds, for example behind a
// feature flag, and the composition root must pick the factory at runtime.
//
// pub struct MockNodesGatewayFactory;
//
// impl NodesGatewayFactory for MockNodesGatewayFactory {
//     type Gateway = MockNodesGateway;
//
//     fn create(&self, _config: &Config) -> Result<Self::Gateway, GatewayFactoryError> {
//         Ok(MockNodesGateway::new())
//     }
// }
