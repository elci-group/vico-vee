//! HTTP server for the standalone vico-vee service.
//!
//! Mirrors the `/vee/*` API surface previously hosted inside vico-desktop so
//! that ViCo can talk to VEE over HTTP without embedding the executor.

use axum::{
    extract::{Json, State},
    response::Json as JsonResponse,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    capability::CapabilityRegistry,
    openapi::{docs, openapi_json},
    types::{
        Capability, ExecutionLanguage, ExecutionTask, OsmosisArtifactRef, OsmosisDiffRequest,
        OsmosisMergeRequest, OsmosisOperation, OsmosisRejectRequest, Provenance,
    },
    ExecutorDaemon,
};

/// Server configuration.
pub use crate::config::Config;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub vee: Arc<ExecutorDaemon>,
    pub capability_issuer: Arc<Mutex<CapabilityRegistry>>,
    pub config: Config,
}

impl AppState {
    pub async fn try_new(config: Config) -> Result<Self, String> {
        std::fs::create_dir_all(&config.data_dir).map_err(|e| format!("create data dir: {}", e))?;

        let key_dir = config.data_dir.join("keys");
        let revocation_dir = config.data_dir.join("revocations");
        let capability_issuer = Arc::new(Mutex::new(
            CapabilityRegistry::try_new_with_key_dir(&key_dir, &revocation_dir).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "failed to create keyring-backed registry; using seeded fallback");
                CapabilityRegistry::new_with_seed([0u8; 32])
            }),
        ));

        let verifier = capability_issuer.lock().await.verifier();
        let vee = Arc::new(
            ExecutorDaemon::try_new_with_verifier(verifier)
                .map_err(|e| format!("executor daemon: {}", e))?,
        );
        vee.start().await;

        Ok(Self {
            vee,
            capability_issuer,
            config,
        })
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", post(health))
        .route("/openapi.json", get(openapi_json))
        .route("/docs", get(docs))
        .route("/vee/submit", post(vee_submit))
        .route("/vee/status", post(vee_status))
        .route("/vee/cancel", post(vee_cancel))
        .route("/vee/list", post(vee_list))
        .route("/vee/artifacts", post(vee_artifacts))
        .route("/vee/dashboard", post(vee_dashboard))
        .route("/vee/patterns", post(vee_patterns))
        .route("/vee/audit", post(vee_audit))
        .route("/vee/checkpoints", post(vee_checkpoints))
        .route("/vee/odin/health", post(vee_odin_health))
        .route("/vee/odin/model", post(vee_odin_set_model))
        .route("/vee/diff", post(vee_diff))
        .route("/vee/merge", post(vee_merge))
        .route("/vee/reject", post(vee_reject))
        .with_state(state)
}

async fn health() -> JsonResponse<serde_json::Value> {
    JsonResponse(serde_json::json!({"status": "ok", "service": "vico-vee"}))
}

#[derive(Debug, Deserialize)]
pub struct VeeSubmitInput {
    run_id: Option<String>,
    agent_id: String,
    language: String,
    source_code: String,
    capabilities: Vec<String>,
    budget: Option<crate::types::VeeBudgetRequest>,
    hypothesis: Option<serde_json::Value>,
}

pub async fn vee_submit(
    State(state): State<AppState>,
    Json(input): Json<VeeSubmitInput>,
) -> JsonResponse<serde_json::Value> {
    let language = match input.language.as_str() {
        "python" => ExecutionLanguage::Python,
        "rust" => ExecutionLanguage::Rust,
        "javascript" => ExecutionLanguage::JavaScript,
        "go" => ExecutionLanguage::Go,
        "bound" | "context_bundle" => ExecutionLanguage::ContextBundle,
        "shell" => ExecutionLanguage::Shell,
        "wasm" => ExecutionLanguage::Wasm,
        _ => ExecutionLanguage::Python,
    };

    let capabilities =
        crate::capability::CapabilityRegistry::parse_capabilities(&input.capabilities);
    let budget = input
        .budget
        .map(crate::types::ExecutionBudget::from)
        .unwrap_or_default();

    let execution_id = format!("vee-{}", uuid::Uuid::new_v4());

    let capability_grants = {
        let mut issuer = state.capability_issuer.lock().await;
        capabilities
            .iter()
            .map(|cap| {
                issuer.grant(
                    &execution_id,
                    cap.clone(),
                    crate::types::GrantAuthority::Orchestrator,
                    None,
                )
            })
            .collect()
    };

    let provenance = Provenance {
        artifact_id: format!("prov-{}", execution_id),
        task_id: execution_id.clone(),
        execution_id: execution_id.clone(),
        creator_agent: input.agent_id.clone(),
        parent_artifacts: vec![],
        code_generator: input.agent_id.clone(),
        executed_code: input.source_code.clone(),
        granted_capabilities: input.capabilities.clone(),
        created_at: chrono::Utc::now(),
        previous_hash: "genesis".into(),
        self_hash: String::new(),
    };

    let task = ExecutionTask {
        execution_id: execution_id.clone(),
        run_id: input.run_id,
        agent_id: input.agent_id,
        language,
        source_code: input.source_code,
        capabilities,
        capability_grants,
        budget,
        hypothesis: input
            .hypothesis
            .and_then(|h| serde_json::from_value(h).ok()),
        provenance,
    };

    match state.vee.submit(task).await {
        Ok(msg) => JsonResponse(serde_json::json!({
            "success": true,
            "execution_id": execution_id,
            "status": "pending",
            "message": msg,
        })),
        Err(e) => JsonResponse(serde_json::json!({
            "success": false,
            "error": e,
        })),
    }
}

#[derive(Debug, Deserialize)]
pub struct VeeExecutionIdInput {
    execution_id: String,
}

pub async fn vee_status(
    State(state): State<AppState>,
    Json(input): Json<VeeExecutionIdInput>,
) -> JsonResponse<serde_json::Value> {
    match state.vee.get_status(&input.execution_id).await {
        Some(result) => JsonResponse(serde_json::json!({ "success": true, "data": result })),
        None => JsonResponse(serde_json::json!({
            "success": false,
            "error": "Execution not found",
        })),
    }
}

pub async fn vee_cancel(
    State(state): State<AppState>,
    Json(input): Json<VeeExecutionIdInput>,
) -> JsonResponse<serde_json::Value> {
    match state.vee.cancel(&input.execution_id).await {
        Ok(()) => JsonResponse(serde_json::json!({
            "success": true,
            "execution_id": input.execution_id,
            "status": "cancelled",
        })),
        Err(e) => JsonResponse(serde_json::json!({
            "success": false,
            "error": e,
        })),
    }
}

#[derive(Debug, Deserialize)]
pub struct VeeListInput {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    _agent_id: Option<String>,
    #[serde(default = "default_vee_limit")]
    limit: usize,
}

pub fn default_vee_limit() -> usize {
    50
}

pub async fn vee_list(
    State(state): State<AppState>,
    Json(input): Json<VeeListInput>,
) -> JsonResponse<serde_json::Value> {
    let status_filter = input.status.and_then(|s| match s.as_str() {
        "pending" => Some(crate::types::ExecutionStatus::Pending),
        "queued" => Some(crate::types::ExecutionStatus::Queued),
        "executing" => Some(crate::types::ExecutionStatus::Executing),
        "completed" => Some(crate::types::ExecutionStatus::Completed),
        "failed" => Some(crate::types::ExecutionStatus::Failed),
        "cancelled" => Some(crate::types::ExecutionStatus::Cancelled),
        _ => None,
    });

    let mut results = state.vee.list(status_filter).await;
    results.truncate(input.limit);

    JsonResponse(serde_json::json!({ "success": true, "data": results }))
}

pub async fn vee_artifacts(
    State(state): State<AppState>,
    Json(input): Json<VeeExecutionIdInput>,
) -> JsonResponse<serde_json::Value> {
    let artifacts = state.vee.get_artifacts(&input.execution_id).await;
    let artifacts_json: Vec<serde_json::Value> = artifacts
        .into_iter()
        .map(|(id, artifact)| {
            serde_json::json!({
                "id": id,
                "artifact": artifact,
            })
        })
        .collect();
    JsonResponse(serde_json::json!({
        "success": true,
        "execution_id": input.execution_id,
        "artifacts": artifacts_json,
    }))
}

pub async fn vee_dashboard(State(state): State<AppState>) -> JsonResponse<serde_json::Value> {
    let stats = state.vee.dashboard_stats().await;
    JsonResponse(serde_json::json!({ "success": true, "data": stats }))
}

#[derive(Debug, Deserialize)]
pub struct VeePatternInput {
    #[serde(default)]
    tag: Option<String>,
    #[serde(default = "default_vee_limit")]
    limit: usize,
}

pub async fn vee_patterns(
    State(state): State<AppState>,
    Json(input): Json<VeePatternInput>,
) -> JsonResponse<serde_json::Value> {
    let patterns = state
        .vee
        .find_patterns(&crate::types::TaskSignature {
            language: crate::types::ExecutionLanguage::Python,
            intent_keywords: input.tag.map(|t| vec![t]).unwrap_or_default(),
            required_capabilities: vec![],
            estimated_complexity: 5,
        })
        .await;

    let limited: Vec<_> = patterns.into_iter().take(input.limit).collect();
    JsonResponse(serde_json::json!({ "success": true, "data": limited }))
}

pub async fn vee_audit(State(state): State<AppState>) -> JsonResponse<serde_json::Value> {
    let report = state.vee.run_audit();
    JsonResponse(serde_json::json!({
        "success": true,
        "data": {
            "overall_pass": report.overall_pass,
            "passed": report.passed_count,
            "failed": report.failed_count,
            "critical_failures": report.critical_failures,
            "timestamp": report.timestamp,
            "tests": report.tests.into_iter().map(|t| serde_json::json!({
                "name": t.test_name,
                "passed": t.passed,
                "severity": format!("{:?}", t.severity),
                "detail": t.detail,
            })).collect::<Vec<_>>(),
        }
    }))
}

pub async fn vee_checkpoints(State(state): State<AppState>) -> JsonResponse<serde_json::Value> {
    match state.vee.checkpoint_stats().await {
        Ok(stats) => JsonResponse(serde_json::json!({ "success": true, "data": stats })),
        Err(e) => JsonResponse(serde_json::json!({ "success": false, "error": e })),
    }
}

pub async fn vee_odin_health(State(state): State<AppState>) -> JsonResponse<serde_json::Value> {
    let healthy = state.vee.odin_health().await;
    let models = state.vee.odin_models().await;
    JsonResponse(serde_json::json!({
        "success": true,
        "data": {
            "healthy": healthy,
            "models": models,
        }
    }))
}

#[derive(Debug, Deserialize)]
pub struct OdinSetModelInput {
    model: String,
}

pub async fn vee_odin_set_model(
    State(state): State<AppState>,
    Json(input): Json<OdinSetModelInput>,
) -> JsonResponse<serde_json::Value> {
    state.vee.set_odin_model(input.model).await;
    JsonResponse(serde_json::json!({ "success": true, "message": "ODIN model updated" }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Osmosis — Patch review lifecycle
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct OsmosisDiffInput {
    pub left_execution_id: String,
    pub left_artifact_id: Option<String>,
    pub right_execution_id: Option<String>,
    pub right_artifact_id: Option<String>,
    pub target_path: Option<String>,
    pub format: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OsmosisMergeInput {
    pub source_execution_id: String,
    pub source_artifact_id: Option<String>,
    pub target_path: String,
    pub strategy: Option<String>,
    pub base_execution_id: Option<String>,
    pub base_artifact_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OsmosisRejectInput {
    pub source_execution_id: String,
    pub source_artifact_id: Option<String>,
    pub target_path: String,
    pub base_execution_id: Option<String>,
    pub base_artifact_id: Option<String>,
    pub reason: Option<String>,
}

async fn submit_osmosis_task(
    state: &AppState,
    agent_id: &str,
    operation: OsmosisOperation,
    capabilities: Vec<Capability>,
) -> Result<String, String> {
    let execution_id = format!("vee-osmosis-{}", uuid::Uuid::new_v4());

    let capability_grants = {
        let mut issuer = state.capability_issuer.lock().await;
        capabilities
            .iter()
            .map(|cap| {
                issuer.grant(
                    &execution_id,
                    cap.clone(),
                    crate::types::GrantAuthority::Orchestrator,
                    None,
                )
            })
            .collect()
    };

    let mut payload = serde_json::to_value(&operation).map_err(|e| e.to_string())?;
    if let Some(root) = std::env::var("VICO_VEE_PROJECT_ROOT").ok() {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("project_root".to_string(), serde_json::Value::String(root));
        }
    }

    let provenance = Provenance {
        artifact_id: format!("prov-{}", execution_id),
        task_id: execution_id.clone(),
        execution_id: execution_id.clone(),
        creator_agent: agent_id.to_string(),
        parent_artifacts: vec![],
        code_generator: agent_id.to_string(),
        executed_code: payload.to_string(),
        granted_capabilities: capabilities.iter().map(|c| c.name().to_string()).collect(),
        created_at: chrono::Utc::now(),
        previous_hash: "genesis".into(),
        self_hash: String::new(),
    };

    let task = ExecutionTask {
        execution_id: execution_id.clone(),
        run_id: None,
        agent_id: agent_id.to_string(),
        language: ExecutionLanguage::Osmosis,
        source_code: payload.to_string(),
        capabilities,
        capability_grants,
        budget: crate::types::ExecutionBudget {
            cpu_seconds: 10,
            memory_mb: 128,
            disk_mb: 10,
            token_budget: 0,
            wall_clock_seconds: 30,
        },
        hypothesis: None,
        provenance,
    };

    state.vee.submit(task).await?;
    Ok(execution_id)
}

pub async fn vee_diff(
    State(state): State<AppState>,
    Json(input): Json<OsmosisDiffInput>,
) -> JsonResponse<serde_json::Value> {
    let format = input.format.as_deref().map(|f| match f {
        "unified" => crate::types::OsmosisDiffFormat::Unified,
        _ => crate::types::OsmosisDiffFormat::Structured,
    });

    let left = OsmosisArtifactRef {
        execution_id: input.left_execution_id,
        artifact_id: input.left_artifact_id,
    };
    let right = input.right_execution_id.map(|id| OsmosisArtifactRef {
        execution_id: id,
        artifact_id: input.right_artifact_id,
    });

    let operation = OsmosisOperation::Diff(OsmosisDiffRequest {
        left,
        right,
        target_path: input.target_path,
        format,
    });

    match submit_osmosis_task(
        &state,
        "osmosis",
        operation,
        vec![Capability::FilesystemRead {
            paths: vec!["*".to_string()],
        }],
    )
    .await
    {
        Ok(execution_id) => JsonResponse(serde_json::json!({
            "success": true,
            "execution_id": execution_id,
            "status": "pending",
        })),
        Err(e) => JsonResponse(serde_json::json!({ "success": false, "error": e })),
    }
}

pub async fn vee_merge(
    State(state): State<AppState>,
    Json(input): Json<OsmosisMergeInput>,
) -> JsonResponse<serde_json::Value> {
    let strategy = input.strategy.as_deref().map(|s| match s {
        "append" => crate::types::OsmosisMergeStrategy::Append,
        _ => crate::types::OsmosisMergeStrategy::Overwrite,
    });

    let source = OsmosisArtifactRef {
        execution_id: input.source_execution_id,
        artifact_id: input.source_artifact_id,
    };
    let base = input.base_execution_id.map(|id| OsmosisArtifactRef {
        execution_id: id,
        artifact_id: input.base_artifact_id,
    });

    let operation = OsmosisOperation::Merge(OsmosisMergeRequest {
        source,
        target_path: input.target_path,
        strategy,
        base,
    });

    let capabilities = vec![
        Capability::FilesystemRead {
            paths: vec!["*".to_string()],
        },
        Capability::FilesystemWrite {
            paths: vec!["*".to_string()],
        },
        Capability::FilesystemCreate {
            paths: vec!["*".to_string()],
        },
    ];

    match submit_osmosis_task(&state, "osmosis", operation, capabilities).await {
        Ok(execution_id) => JsonResponse(serde_json::json!({
            "success": true,
            "execution_id": execution_id,
            "status": "pending",
        })),
        Err(e) => JsonResponse(serde_json::json!({ "success": false, "error": e })),
    }
}

pub async fn vee_reject(
    State(state): State<AppState>,
    Json(input): Json<OsmosisRejectInput>,
) -> JsonResponse<serde_json::Value> {
    let source = OsmosisArtifactRef {
        execution_id: input.source_execution_id,
        artifact_id: input.source_artifact_id,
    };
    let base = input.base_execution_id.map(|id| OsmosisArtifactRef {
        execution_id: id,
        artifact_id: input.base_artifact_id,
    });

    let operation = OsmosisOperation::Reject(OsmosisRejectRequest {
        source,
        target_path: input.target_path,
        base,
        reason: input.reason,
    });

    let capabilities = vec![
        Capability::FilesystemRead {
            paths: vec!["*".to_string()],
        },
        Capability::FilesystemWrite {
            paths: vec!["*".to_string()],
        },
        Capability::FilesystemCreate {
            paths: vec!["*".to_string()],
        },
    ];

    match submit_osmosis_task(&state, "osmosis", operation, capabilities).await {
        Ok(execution_id) => JsonResponse(serde_json::json!({
            "success": true,
            "execution_id": execution_id,
            "status": "pending",
        })),
        Err(e) => JsonResponse(serde_json::json!({ "success": false, "error": e })),
    }
}
