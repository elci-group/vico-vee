//! Core worker abstractions and factory.
//!
//! Defines the `RuntimeWorker` trait, the reusable `WorkerPool`, and the
//! `create_worker` factory used by the executor daemon.

use crate::capability::CapabilityVerifier;
use crate::types::{
    Capability, CapabilityGrant, ExecutionBudget, ExecutionError, ExecutionLanguage,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A worker capable of executing tasks in a specific language.
#[async_trait]
pub trait RuntimeWorker: Send + Sync {
    /// Initialise the worker with signed capability grants and limits.
    ///
    /// Workers verify the grants themselves rather than trusting the daemon.
    async fn init(
        &mut self,
        execution_id: &str,
        grants: Vec<CapabilityGrant>,
        verifier: Arc<CapabilityVerifier>,
        required_capabilities: Vec<Capability>,
        budget: ExecutionBudget,
    ) -> Result<(), String>;

    /// Execute a task and return artifacts.
    async fn execute(
        &self,
        task: &crate::types::ExecutionTask,
    ) -> Result<Vec<crate::types::Artifact>, ExecutionError>;

    /// Gracefully shut down the worker.
    async fn shutdown(self: Box<Self>) -> Result<(), String>;
}

/// Verify grants cover the requested capabilities and return the granted capabilities.
pub(crate) fn verify_task_grants(
    execution_id: &str,
    grants: &[CapabilityGrant],
    verifier: &CapabilityVerifier,
    required: &[Capability],
) -> Result<Vec<Capability>, String> {
    verifier.verify_grants_for_task(execution_id, grants, required)
}

/// Worker pool for reusable worker instances.
pub struct WorkerPool {
    workers: Arc<Mutex<Vec<Box<dyn RuntimeWorker>>>>,
}

impl WorkerPool {
    pub fn new() -> Self {
        Self {
            workers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn claim(&self) -> Option<Box<dyn RuntimeWorker>> {
        self.workers.lock().await.pop()
    }

    pub async fn return_worker(&self, worker: Box<dyn RuntimeWorker>) {
        self.workers.lock().await.push(worker);
    }
}

impl Default for WorkerPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve a helper tool binary from environment, PATH, or a default checkout.
pub(crate) fn tool_binary(env_var: &str, binary: &str, checkout_dir: &str) -> String {
    if let Ok(path) = std::env::var(env_var) {
        return path;
    }
    if executable_present(binary) {
        return binary.to_string();
    }
    if let Some(home) = dirs::home_dir() {
        let checkout_binary = home
            .join(checkout_dir)
            .join("target")
            .join("release")
            .join(binary);
        if checkout_binary.exists() {
            return checkout_binary.to_string_lossy().to_string();
        }
    }
    binary.to_string()
}

/// Check whether a binary is present on PATH.
pub(crate) fn executable_present(binary: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|path| path.join(binary).exists()))
        .unwrap_or(false)
}

/// Factory to create a worker for a given language.
pub fn create_worker(
    language: ExecutionLanguage,
    artifact_store: Arc<crate::artifact::ArtifactStore>,
) -> Box<dyn RuntimeWorker> {
    match language {
        ExecutionLanguage::Python => Box::new(super::python::PythonWorker::new()),
        ExecutionLanguage::Go => Box::new(super::go_tools::GoWorker::new()),
        ExecutionLanguage::ContextBundle => Box::new(super::go_tools::ContextBundleWorker::new()),
        ExecutionLanguage::Shell => Box::new(super::shell::ShellWorker::new()),
        ExecutionLanguage::Osmosis => Box::new(super::osmosis::OsmosisWorker::new(artifact_store)),
        _ => Box::new(super::python::PythonWorker::new()),
    }
}
