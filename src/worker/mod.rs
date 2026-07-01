//! Runtime Workers
//!
//! Multi-language worker trait and subprocess-based implementations.
//! Phase 2: Real subprocess execution with Landlock + seccomp-bpf + rlimit.

mod core;
mod go_tools;
mod osmosis;
mod python;
mod shell;

pub use core::{create_worker, RuntimeWorker, WorkerPool};
pub use go_tools::{ContextBundleWorker, GoWorker};
pub use osmosis::OsmosisWorker;
pub use python::PythonWorker;
pub use shell::ShellWorker;
