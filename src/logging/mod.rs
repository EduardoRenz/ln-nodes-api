//! Import-run logging: a pluggable audit sink.
//!
//! [`run_logger::RunLogger`] is the port the application records import runs
//! through; the sibling modules are the implementations (today: the file
//! backend) and the runtime selection between them. This is deliberately
//! *not* a leveled logger — `tracing` owns diagnostics (`RUST_LOG`, console,
//! future subscriber-based export), while this module owns the one-line
//! audit record written for every run.

pub mod any_run_logger;
pub mod file_run_logger;
pub mod file_sink;
pub mod run_logger;
pub mod run_logger_factory;
