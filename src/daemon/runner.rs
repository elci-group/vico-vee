use crate::artifact::ArtifactStore;
use crate::types::*;
use crate::worker::create_worker;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::Inner;

/// Background worker that drives a single execution through its lifecycle.
pub(crate) async fn run_execution(
    inner: Arc<Inner>,
    task: ExecutionTask,
    token: CancellationToken,
) {
    let execution_id = task.execution_id.clone();
    let started_at = Utc::now();

    // Transition to Executing.
    {
        let mut store = inner.store.write().await;
        if let Some(result) = store.get_mut(&execution_id) {
            result.status = ExecutionStatus::Executing;
            result.phase = ExecutionPhase::Execution;
            result.started_at = Some(started_at);
        }
    }
    emit_event(
        &inner,
        "started",
        &execution_id,
        json!({
            "agent_id": task.agent_id,
            "language": task.language.to_string(),
            "capabilities": task.capabilities.iter().map(|c| c.name()).collect::<Vec<_>>(),
        }),
    );

    if token.is_cancelled() {
        mark_cancelled(&inner, &execution_id).await;
        inner.inflight.lock().await.remove(&execution_id);
        return;
    }

    // Build worker and artifact store.
    let artifact_store = Arc::new(ArtifactStore::default());
    let mut worker = create_worker(task.language.clone(), artifact_store.clone());

    if let Err(e) = worker
        .init(
            &execution_id,
            task.capability_grants.clone(),
            Arc::new(inner.verifier.clone()),
            task.capabilities.clone(),
            task.budget.clone(),
        )
        .await
    {
        mark_failed(&inner, &execution_id, format!("worker init failed: {}", e)).await;
        inner.inflight.lock().await.remove(&execution_id);
        return;
    }

    if token.is_cancelled() {
        mark_cancelled(&inner, &execution_id).await;
        inner.inflight.lock().await.remove(&execution_id);
        return;
    }

    // Execute with cooperative cancellation.
    let execute_fut = worker.execute(&task);
    let result = tokio::select! {
        biased;
        _ = token.cancelled() => {
            mark_cancelled(&inner, &execution_id).await;
            inner.inflight.lock().await.remove(&execution_id);
            return;
        }
        res = execute_fut => res,
    };

    match result {
        Ok(artifacts) => {
            let completed_at = Utc::now();
            let latency_ms = (completed_at - started_at)
                .num_milliseconds()
                .max(0)
                .unsigned_abs();

            // Extract telemetry from the sandbox metadata artifact if present.
            let mut cpu_seconds_used = 0.0f64;
            let mut memory_peak_mb = 0.0f64;
            for artifact in &artifacts {
                if let Artifact::Json { value, .. } = artifact {
                    if let Some(ms) = value.get("duration_ms").and_then(|v| v.as_u64()) {
                        cpu_seconds_used = ms as f64 / 1000.0;
                    }
                    if let Some(kb) = value.get("memory_peak_kb").and_then(|v| v.as_u64()) {
                        memory_peak_mb = kb as f64 / 1024.0;
                    }
                }
            }

            // Persist artifacts and keep them on the result.
            let mut provenance = task.provenance.clone();
            provenance.execution_id = execution_id.clone();
            let mut stored_artifacts = Vec::with_capacity(artifacts.len());
            for artifact in artifacts {
                // Persistence is best-effort; the artifact is still part of the
                // execution result even if the store write fails.
                let _ = artifact_store
                    .store(artifact.clone(), Some(provenance.clone()))
                    .await;
                stored_artifacts.push(artifact);
            }
            let artifact_count = stored_artifacts.len();

            {
                let mut store = inner.store.write().await;
                if let Some(result) = store.get_mut(&execution_id) {
                    result.status = ExecutionStatus::Completed;
                    result.phase = ExecutionPhase::Validation;
                    result.artifacts = stored_artifacts;
                    result.completed_at = Some(completed_at);
                    result.latency_ms = latency_ms;
                    result.cpu_seconds_used = cpu_seconds_used;
                    result.memory_peak_mb = memory_peak_mb;
                }
            }
            emit_event(
                &inner,
                "completed",
                &execution_id,
                json!({
                    "latency_ms": latency_ms,
                    "cpu_seconds_used": cpu_seconds_used,
                    "artifact_count": artifact_count,
                }),
            );
        }
        Err(err) => {
            mark_failed(
                &inner,
                &execution_id,
                format!("{}: {}", err.code, err.message),
            )
            .await;
        }
    }

    inner.inflight.lock().await.remove(&execution_id);
}

async fn mark_failed(inner: &Inner, execution_id: &str, message: String) {
    let completed_at = Utc::now();
    let latency_ms = inner
        .store
        .read()
        .await
        .get(execution_id)
        .and_then(|r| r.started_at)
        .map(|s| (completed_at - s).num_milliseconds().max(0).unsigned_abs())
        .unwrap_or(0);

    {
        let mut store = inner.store.write().await;
        if let Some(result) = store.get_mut(execution_id) {
            result.status = ExecutionStatus::Failed;
            result.error_log = Some(message.clone());
            result.completed_at = Some(completed_at);
            result.latency_ms = latency_ms;
        }
    }
    emit_event(
        inner,
        "failed",
        execution_id,
        json!({ "error": message, "latency_ms": latency_ms }),
    );
}

async fn mark_cancelled(inner: &Inner, execution_id: &str) {
    {
        let mut store = inner.store.write().await;
        if let Some(result) = store.get_mut(execution_id) {
            result.status = ExecutionStatus::Cancelled;
            result.completed_at = Some(Utc::now());
        }
    }
    emit_event(inner, "cancelled", execution_id, json!({}));
}

fn emit_event(inner: &Inner, event: &str, execution_id: &str, payload: serde_json::Value) {
    let _ = inner.event_tx.send(json!({
        "event": event,
        "execution_id": execution_id,
        "timestamp": Utc::now().to_rfc3339(),
        "payload": payload,
    }));
}
