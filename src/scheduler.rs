use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use crate::application::import_nodes::{ImportOutcome, import_nodes};
use crate::gateways::nodes_gateway::NodesGateway;
use crate::logging::run_logger::RunLogger;
use crate::repositories::nodes_repository::NodesRepository;

pub async fn run<G, R, L>(
    gateway: Arc<G>,
    repository: Arc<R>,
    interval: Duration,
    logger: L,
    mut shutdown: watch::Receiver<bool>,
) where
    G: NodesGateway + 'static,
    R: NodesRepository + 'static,
    L: RunLogger + 'static,
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
                let outcome = import_nodes(gateway.as_ref(), repository.as_ref()).await;
                logger.record_or_warn(&outcome).await;
                match outcome {
                    Ok(ImportOutcome::Imported { count, new }) => {
                        tracing::info!(count, new, "imported node snapshot");
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
