//! VEE — ViCo Execution Environment
//!
//! A deterministic cognitive substrate for agent reasoning.
//! Treats executable transformations as first-class cognitive operations.
//!
//! The runtime is end-to-end persistent: SQLite metadata for tasks,
//! checkpoints, and pattern history, plus a content-addressable filesystem
//! store for artifacts. Execution is isolated through rlimit, Landlock, and
//! seccomp-bpf with real subprocess workers. Capabilities are signed by the
//! orchestrator using Ed25519 keys backed by the OS keyring.

pub mod artifact;
pub mod audit;
pub mod capability;
pub mod checkpoint;
pub mod config;
pub mod daemon;
pub mod auth;
pub mod migrations;
pub mod openapi;
pub mod osmosis;
pub mod paths;
pub mod pattern;
pub mod provenance;
pub mod sandbox;
pub mod server;
pub mod types;
pub mod validation;
pub mod worker;

#[cfg(test)]
pub mod tests;

pub use artifact::ArtifactStore;
pub use capability::{CapabilityRegistry, GrantAuthority};
pub use daemon::ExecutorDaemon;
pub use pattern::PatternStore;
pub use types::*;
