//! Sandbox Runtime
//!
//! Layered isolation for agent code execution:
//!   Layer 1: rlimit — CPU time, memory, file size, process count
//!   Layer 2: Landlock LSM — filesystem confinement (Linux 5.13+)
//!   Layer 3: seccomp-bpf — syscall allowlist
//!
//! Each layer is best-effort: if a layer is unavailable, execution continues
//! with the remaining layers. This ensures portability across kernels.

mod artifacts;
mod core;
mod landlock;
mod seccomp;
mod types;

pub use artifacts::{build_python_command, extract_artifacts};
pub use core::{apply_sandbox_pre_exec, run_sandboxed};
pub use types::{SandboxConfig, SandboxResult};
