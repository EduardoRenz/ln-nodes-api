use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use crate::application::import_nodes::{ImportError, ImportOutcome, import_nodes};
use crate::gateways::nodes_gateway::NodesGateway;
use crate::infrastructure::import_log;
use crate::repositories::nodes_repository::NodesRepository;

pub async fn run<G, R>(
    gateway: Arc<G>,
    repository: Arc<R>,
    interval: Duration,
    log_dir: PathBuf,
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
                let outcome = import_nodes(gateway.as_ref(), repository.as_ref()).await;
                record_run(&log_dir, &outcome);
                match outcome {
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

/// Appends the run to the date-partitioned log file.
///
/// A write failure must never interrupt the import loop, so it degrades to a
/// warning instead of propagating.
fn record_run(log_dir: &Path, outcome: &Result<ImportOutcome, ImportError>) {
    if let Err(error) = import_log::record(log_dir, Utc::now(), outcome) {
        tracing::warn!(%error, dir = %log_dir.display(), "failed to record import run");
    }
}
