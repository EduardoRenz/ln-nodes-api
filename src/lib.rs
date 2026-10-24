pub mod application;
pub mod composition;
pub mod config;
pub mod domain;
pub mod gateways;
pub mod repositories;
pub mod routes;
pub mod scheduler;
pub mod telemetry;

pub(crate) mod infrastructure;

// Exposed for the binary's one-shot `import` command, which appends to the
// same date-partitioned log the scheduler writes. The rest of
// `infrastructure` stays crate-private.
pub use infrastructure::import_log;
