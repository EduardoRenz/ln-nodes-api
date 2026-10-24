use std::error::Error;
use std::net::SocketAddr;

use tokio::net::TcpListener;
use tokio::sync::watch;

use rust_lightning_service::application::import_nodes::{ImportOutcome, import_nodes};
use rust_lightning_service::composition::{build_app, run_migrations};
use rust_lightning_service::config::Config;
use rust_lightning_service::gateways::nodes_gateway_factory::{
    MempoolNodesGatewayFactory, NodesGatewayFactory,
};
use rust_lightning_service::logging::run_logger::RunLogger;
use rust_lightning_service::logging::run_logger_factory::{
    ConfiguredRunLoggerFactory, RunLoggerFactory,
};
use rust_lightning_service::repositories::nodes_repository_factory::{
    ConfiguredNodesRepositoryFactory, NodesRepositoryFactory,
};
use rust_lightning_service::{scheduler, telemetry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Serve,
    Migrate,
    Import,
    Help,
}

fn parse_command(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let command = args.next();
    let extra = args.next();

    match (command.as_deref(), extra) {
        (None, None) => Ok(Command::Serve),
        (Some("migrate"), None) => Ok(Command::Migrate),
        (Some("import"), None) => Ok(Command::Import),
        (Some("-h" | "--help" | "help"), None) => Ok(Command::Help),
        (Some(other), None) => Err(format!("unknown command: {other}")),
        _ => Err("too many arguments".to_string()),
    }
}

fn print_usage() {
    println!("Usage: rust-lightning-service [COMMAND]");
    println!();
    println!("Commands:");
    println!("  import   Fetch nodes from the ranking API once, persist the snapshot, and exit");
    println!("  migrate  Apply pending database migrations and exit");
    println!("  help     Show this help");
    println!();
    println!("With no command, starts the HTTP server and the importer.");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let command = match parse_command(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            std::process::exit(2);
        }
    };

    if command == Command::Help {
        print_usage();
        return Ok(());
    }

    let config = Config::from_env()?;
    telemetry::init(&config.log_filter);
    let logger = ConfiguredRunLoggerFactory.create(&config);

    if command == Command::Migrate {
        tracing::info!("applying database migrations");
        run_migrations(&config).await?;
        tracing::info!("database migrations applied");
        return Ok(());
    }

    if command == Command::Import {
        return run_import(&config, &logger).await;
    }

    tracing::info!(
        port = config.port,
        persistence = ?config.repository_type,
        interval_secs = config.import_interval.as_secs(),
        "starting service"
    );

    let gateway = MempoolNodesGatewayFactory.create(&config)?;
    let repository = ConfiguredNodesRepositoryFactory.create(&config).await?;
    let app = build_app(gateway, repository, &config);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let importer = tokio::spawn(scheduler::run(
        app.gateway.clone(),
        app.repository.clone(),
        app.import_interval,
        logger,
        shutdown_rx,
    ));

    let listener = TcpListener::bind(("0.0.0.0", config.port)).await?;
    tracing::info!(port = config.port, "http server listening");

    // `into_make_service_with_connect_info` is what populates the client
    // address on each request; without it the rate limiter falls back to a
    // single shared bucket for every caller.
    axum::serve(
        listener,
        app.router
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    let _ = shutdown_tx.send(true);
    let _ = importer.await;

    Ok(())
}

/// One-shot import: fetch once, persist, record the run, and report.
///
/// The repository factory runs pending migrations, so a bare import against
/// a fresh database works without a separate migrate step. Exit semantics:
/// a successful or skipped import is `Ok`; a fetch or database failure
/// propagates so the process exits non-zero.
async fn run_import(config: &Config, logger: &impl RunLogger) -> Result<(), Box<dyn Error>> {
    let gateway = MempoolNodesGatewayFactory.create(config)?;
    let repository = ConfiguredNodesRepositoryFactory.create(config).await?;
    let outcome = import_nodes(&gateway, &repository).await;

    logger.record_or_warn(&outcome).await;

    match outcome {
        Ok(ImportOutcome::Imported { count, new }) => {
            tracing::info!(count, new, "imported node snapshot");
            Ok(())
        }
        Ok(ImportOutcome::SkippedEmpty) => {
            tracing::warn!("mempool returned no valid nodes; keeping previous snapshot");
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "failed to install SIGTERM handler");
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_command(args.iter().map(|arg| (*arg).to_string()))
    }

    #[test]
    fn no_arguments_starts_the_server() {
        assert_eq!(parse(&[]), Ok(Command::Serve));
    }

    #[test]
    fn parses_migrate_and_import() {
        assert_eq!(parse(&["migrate"]), Ok(Command::Migrate));
        assert_eq!(parse(&["import"]), Ok(Command::Import));
    }

    #[test]
    fn parses_help_aliases() {
        assert_eq!(parse(&["help"]), Ok(Command::Help));
        assert_eq!(parse(&["-h"]), Ok(Command::Help));
        assert_eq!(parse(&["--help"]), Ok(Command::Help));
    }

    #[test]
    fn rejects_unknown_command() {
        assert_eq!(parse(&["serve"]), Err("unknown command: serve".to_string()));
    }

    #[test]
    fn rejects_extra_arguments() {
        assert_eq!(
            parse(&["import", "extra"]),
            Err("too many arguments".to_string())
        );
        assert_eq!(
            parse(&["migrate", "extra"]),
            Err("too many arguments".to_string())
        );
    }
}
