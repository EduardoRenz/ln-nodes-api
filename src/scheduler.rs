use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use crate::application::import_nodes::{ImportOutcome, import_nodes};
use crate::gateways::nodes_gateway::NodesGateway;
use crate::repositories::nodes_repository::NodesRepository;

pub async fn run<G, R>(
    gateway: Arc<G>,
    repository: Arc<R>,
    interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) where
    G: NodesGateway + 'static,
    R: NodesRepository + 'static,
{
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                tracing::info!("import scheduler shutting down");
                break;
            }
            _ = ticker.tick() => {
                match import_nodes(gateway.as_ref(), repository.as_ref()).await {
                    Ok(ImportOutcome::Imported { count }) => {
                        tracing::info!(count, "imported node snapshot");
                    }
                    Ok(ImportOutcome::SkippedEmpty) => {
                        tracing::warn!("mempool returned no valid nodes; keeping previous snapshot");
                    }
                    Err(error) => {
                        tracing::error!(%error, "import failed; keeping previous snapshot");
                    }
                }
            }
        }
    }
}
