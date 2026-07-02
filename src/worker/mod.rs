//! Runtime Workers
//!
//! Multi-language worker trait and subprocess-based implementations.
//! Phase 2: Real subprocess execution with Landlock + seccomp-bpf + rlimit.

mod core;
mod go_tools;
mod javascript;
mod osmosis;
mod python;
mod rust;
mod shell;
mod wasm;

pub use core::{create_worker, RuntimeWorker, WorkerOutput, WorkerPool};
pub use go_tools::{ContextBundleWorker, GoWorker};
pub use javascript::JavaScriptWorker;
pub use osmosis::OsmosisWorker;
pub use python::PythonWorker;
pub use rust::RustWorker;
pub use shell::ShellWorker;
pub use wasm::WasmWorker;
