//! Executor Daemon (vicod)
//!
//! Core execution engine, task queue, sandbox management, and event broadcasting.
//! Tasks are verified, executed by language-specific workers in a background
//! tokio task, and their results are persisted in memory. Artifacts produced by
//! workers are stored in the persistent ArtifactStore when possible.

use crate::capability::{CapabilityRegistry, CapabilityVerifier};
use crate::types::*;
use chrono::Utc;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

mod audit;
mod odin;
mod patterns;
mod runner;

pub use audit::{AuditReport, AuditTest};
pub use patterns::PatternRecord;

/// The central executor daemon.
#[derive(Clone)]
pub struct ExecutorDaemon {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) store: RwLock<HashMap<String, ExecutionResult>>,
    pub(crate) verifier: std::sync::RwLock<CapabilityVerifier>,
    pub(crate) cancel: CancellationToken,
    pub(crate) handle: Mutex<Option<JoinHandle<()>>>,
    pub(crate) event_tx: broadcast::Sender<serde_json::Value>,
    pub(crate) inflight: Mutex<HashMap<String, (CancellationToken, JoinHandle<()>)>>,
}

impl ExecutorDaemon {
    /// Create a daemon using a deterministic in-memory capability verifier.
    pub fn try_new() -> Result<Self, String> {
        let registry = CapabilityRegistry::new_with_seed([0u8; 32]);
        Self::try_new_with_verifier(registry.verifier())
    }

    /// Create a daemon with an explicit capability verifier.
    pub fn try_new_with_verifier(verifier: CapabilityVerifier) -> Result<Self, String> {
        Ok(Self::with_verifier(verifier))
    }

    /// Synchronous constructor for callers that do not need async setup.
    pub fn new() -> Self {
        let registry = CapabilityRegistry::new_with_seed([0u8; 32]);
        Self::with_verifier(registry.verifier())
    }

    fn with_verifier(verifier: CapabilityVerifier) -> Self {
        let (event_tx, _event_rx) = broadcast::channel(128);
        Self {
            inner: Arc::new(Inner {
                store: RwLock::new(HashMap::new()),
                verifier: std::sync::RwLock::new(verifier),
                cancel: CancellationToken::new(),
                handle: Mutex::new(None),
                event_tx,
                inflight: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Start the daemon's background task pump.
    pub async fn start(&self) {
        let cancel = self.inner.cancel.clone();
        let new_handle = tokio::spawn(async move {
            // No-op background pump; exits when the daemon is stopped.
            cancel.cancelled().await;
        });

        let mut guard = self.inner.handle.lock().await;
        if let Some(old) = guard.take() {
            old.abort();
        }
        *guard = Some(new_handle);
    }

    /// Stop the daemon and wait for the background pump to finish.
    pub async fn stop(&self) {
        self.inner.cancel.cancel();
        // Abort any in-flight executions.
        let inflight = self.inner.inflight.lock().await.drain().collect::<Vec<_>>();
        for (_id, (token, handle)) in inflight {
            token.cancel();
            handle.abort();
        }
        if let Some(handle) = self.inner.handle.lock().await.take() {
            let _ = handle.await;
        }
    }

    /// Submit a task for execution after verifying its capability grants.
    pub async fn submit(&self, task: ExecutionTask) -> Result<String, String> {
        let verifier = self
            .inner
            .verifier
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Err(e) = verifier.verify_grants_for_task(
            &task.execution_id,
            &task.capability_grants,
            &task.capabilities,
        ) {
            return Err(format!("missing or invalid capability grant: {}", e));
        }

        let result = ExecutionResult {
            execution_id: task.execution_id.clone(),
            status: ExecutionStatus::Queued,
            phase: ExecutionPhase::Hypothesis,
            artifacts: vec![],
            validation: None,
            confidence: 0.0,
            tokens_consumed: 0,
            cpu_seconds_used: 0.0,
            memory_peak_mb: 0.0,
            latency_ms: 0,
            error_log: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };

        self.inner
            .store
            .write()
            .await
            .insert(task.execution_id.clone(), result);

        // Spawn a background worker for this execution and track it so it can
        // be cancelled.
        let execution_id = task.execution_id.clone();
        let token = CancellationToken::new();
        let inner = self.inner.clone();
        let task = task.clone();
        let task_token = token.clone();
        let handle = tokio::spawn(async move {
            runner::run_execution(inner, task, task_token).await;
        });
        self.inner
            .inflight
            .lock()
            .await
            .insert(execution_id, (token, handle));

        Ok("submitted".to_string())
    }

    /// Return the current execution result for an execution id, if any.
    pub async fn get_status(&self, execution_id: &str) -> Option<ExecutionResult> {
        self.inner.store.read().await.get(execution_id).cloned()
    }

    /// Cancel an execution, aborting its worker if it is still in flight and
    /// marking it as `Cancelled`.
    pub async fn cancel(&self, execution_id: &str) -> Result<(), String> {
        if let Some((token, handle)) = self.inner.inflight.lock().await.remove(execution_id) {
            token.cancel();
            handle.abort();
        }

        let mut store = self.inner.store.write().await;
        match store.get_mut(execution_id) {
            Some(result) => {
                result.status = ExecutionStatus::Cancelled;
                result.completed_at = Some(Utc::now());
                Ok(())
            }
            None => Err(format!("execution '{}' not found", execution_id)),
        }
    }

    /// List execution results, optionally filtered by status.
    pub async fn list(&self, filter: Option<ExecutionStatus>) -> Vec<ExecutionResult> {
        self.inner
            .store
            .read()
            .await
            .values()
            .filter(|r| filter.as_ref().is_none_or(|f| r.status == *f))
            .cloned()
            .collect()
    }

    /// Return artifact summaries for an execution.
    pub async fn get_artifacts(&self, execution_id: &str) -> Vec<(String, ArtifactSummary)> {
        let store = self.inner.store.read().await;
        let Some(result) = store.get(execution_id) else {
            return vec![];
        };
        result
            .artifacts
            .iter()
            .map(|a| {
                let summary = ArtifactSummary::from(a);
                (summary.artifact_id.clone(), summary)
            })
            .collect()
    }

    /// Return lightweight dashboard statistics.
    pub async fn dashboard_stats(&self) -> serde_json::Value {
        let store = self.inner.store.read().await;
        let total = store.len() as i64;
        let completed = store
            .values()
            .filter(|r| r.status == ExecutionStatus::Completed)
            .count() as i64;
        let failed = store
            .values()
            .filter(|r| r.status == ExecutionStatus::Failed)
            .count() as i64;
        let pending = store
            .values()
            .filter(|r| {
                matches!(
                    r.status,
                    ExecutionStatus::Pending | ExecutionStatus::Queued | ExecutionStatus::Executing
                )
            })
            .count() as i64;
        let avg_latency_ms = if total > 0 {
            store.values().map(|r| r.latency_ms as i64).sum::<i64>() / total
        } else {
            0
        };

        json!({
            "total": total,
            "completed": completed,
            "failed": failed,
            "pending": pending,
            "avg_latency_ms": avg_latency_ms,
        })
    }

    /// Subscribe to executor events broadcast channel.
    pub fn subscribe_events(&self) -> broadcast::Receiver<serde_json::Value> {
        self.inner.event_tx.subscribe()
    }

    /// Replace the capability verifier used for new executions.
    pub fn update_verifier(&self, verifier: CapabilityVerifier) {
        let mut guard = self.inner.verifier.write().unwrap_or_else(|e| e.into_inner());
        *guard = verifier;
    }
}

impl Default for ExecutorDaemon {
    fn default() -> Self {
        Self::new()
    }
}
