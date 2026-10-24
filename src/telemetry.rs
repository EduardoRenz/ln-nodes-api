use tracing_subscriber::EnvFilter;

pub fn init(filter: &str) {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(filter))
        .finish();

    if let Err(error) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("failed to initialize tracing: {error}");
    }
}
