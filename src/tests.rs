//! VEE Unit Tests

use crate::capability::{CapabilityRegistry, GrantAuthority};
use crate::osmosis::{diff_text, OsmosisEngine};
use crate::pattern::PatternStore;
use crate::provenance;
use crate::types::*;
use crate::worker::RuntimeWorker;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn test_capability_names() {
    let cap = Capability::FilesystemRead {
        paths: vec!["/tmp".into()],
    };
    assert_eq!(cap.name(), "filesystem_read");

    let cap = Capability::NetworkAccess {
        hosts: vec![],
        ports: vec![],
    };
    assert_eq!(cap.name(), "network_access");

    let cap = Capability::GpuCompute { device: None };
    assert_eq!(cap.name(), "gpu_compute");
}

#[test]
fn test_capability_registry_grant_and_verify() {
    let mut registry = CapabilityRegistry::new_with_seed([1u8; 32]);
    let cap = Capability::FilesystemRead {
        paths: vec!["/workspace".into()],
    };

    registry.grant(
        "exec-001",
        cap.clone(),
        GrantAuthority::Orchestrator,
        Some("test".into()),
    );
    assert!(registry.verify("exec-001", "filesystem_read"));
    assert!(!registry.verify("exec-001", "network_access"));
    assert!(!registry.verify("exec-002", "filesystem_read"));
}

#[test]
fn test_capability_registry_parse() {
    let caps = vec![
        "filesystem_read".into(),
        "network_access".into(),
        "unknown_cap".into(),
    ];
    let parsed = CapabilityRegistry::parse_capabilities(&caps);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].name(), "filesystem_read");
    assert_eq!(parsed[1].name(), "network_access");
}

#[test]
fn test_capability_registry_parse_scoped_values() {
    let caps = vec![
        "filesystem_read:/home/sal/goten,/home/sal/egor".into(),
        "filesystem_write:/tmp/vico-out".into(),
        "inference_provider:ollama".into(),
    ];
    let parsed = CapabilityRegistry::parse_capabilities(&caps);

    assert_eq!(
        parsed[0],
        Capability::FilesystemRead {
            paths: vec!["/home/sal/goten".into(), "/home/sal/egor".into()]
        }
    );
    assert_eq!(
        parsed[1],
        Capability::FilesystemWrite {
            paths: vec!["/tmp/vico-out".into()]
        }
    );
    assert_eq!(
        parsed[2],
        Capability::InferenceProvider {
            provider: "ollama".into()
        }
    );
}

#[test]
fn test_pattern_store_seed() {
    let store = PatternStore::new();
    let pattern = store.get("#1044");
    assert!(pattern.is_some());
    let p = pattern.unwrap();
    assert_eq!(p.description, "Clean and sort a CSV dataset");
    assert!(p.success_rate > 0.9);
}

#[test]
fn test_pattern_find_matches() {
    let store = PatternStore::new();
    let sig = TaskSignature {
        language: ExecutionLanguage::Python,
        intent_keywords: vec!["csv".into(), "clean".into()],
        required_capabilities: vec!["filesystem_read".into()],
        estimated_complexity: 3,
    };
    let matches = store.find_matches(&sig, 0.5);
    assert!(!matches.is_empty());
    assert_eq!(matches[0].pattern_id, "#1044");
}

#[test]
fn test_pattern_record_success() {
    let store = PatternStore::new();
    store.record_success("#1044", 1000, 500);
    let p = store.get("#1044").unwrap();
    assert_eq!(p.usage_count, 1);
    assert_eq!(p.success_rate, 1.0);

    store.record_failure("#1044");
    let p = store.get("#1044").unwrap();
    assert_eq!(p.usage_count, 2);
    assert!(p.success_rate < 1.0);
}

#[test]
fn test_provenance_hash_chain() {
    let prov = provenance::build_provenance(
        "art-001",
        "exec-001",
        "task-001",
        "agent-A",
        vec!["art-parent".into()],
        "gpt-4",
        "print('hello')",
        vec!["filesystem_read".into()],
        "genesis",
    );
    assert!(!prov.self_hash.is_empty());
    assert!(provenance::verify_provenance(&prov));
}

#[test]
fn test_provenance_tamper_detection() {
    let mut prov = provenance::build_provenance(
        "art-001",
        "exec-001",
        "task-001",
        "agent-A",
        vec![],
        "gpt-4",
        "print('hello')",
        vec![],
        "genesis",
    );
    assert!(provenance::verify_provenance(&prov));

    // Tamper with the record
    prov.executed_code = "print('tampered')".into();
    assert!(!provenance::verify_provenance(&prov));
}

#[test]
fn test_execution_budget_default() {
    let budget = ExecutionBudget::default();
    assert_eq!(budget.cpu_seconds, 0);
    assert_eq!(budget.memory_mb, 0);
}

#[test]
fn test_vee_budget_request_conversion() {
    let req = VeeBudgetRequest {
        cpu_seconds: Some(60),
        memory_mb: Some(1024),
        disk_mb: Some(200),
        token_budget: Some(10000),
        wall_clock_seconds: Some(120),
    };
    let budget: ExecutionBudget = req.into();
    assert_eq!(budget.cpu_seconds, 60);
    assert_eq!(budget.memory_mb, 1024);
    assert_eq!(budget.disk_mb, 200);
    assert_eq!(budget.token_budget, 10000);
    assert_eq!(budget.wall_clock_seconds, 120);
}

#[test]
fn test_checkpoint_store() {
    use crate::checkpoint::CheckpointStore;
    let store = CheckpointStore::new(&PathBuf::from(":memory:")).unwrap();

    assert_eq!(store.count().unwrap(), 0);

    let ckpt = crate::checkpoint::Checkpoint {
        checkpoint_id: "ckpt-001".into(),
        execution_id: "exec-001".into(),
        phase: ExecutionPhase::Execution,
        status: ExecutionStatus::Executing,
        artifacts_json: "[]".into(),
        validation_json: None,
        error_log: None,
        confidence: 0.5,
        tokens_consumed: 100,
        cpu_seconds_used: 1.0,
        memory_peak_mb: 256.0,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    store.save(&ckpt).unwrap();
    assert_eq!(store.count().unwrap(), 1);

    let latest = store.get_latest("exec-001");
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().phase, ExecutionPhase::Execution);

    let incomplete = store.list_incomplete().unwrap();
    assert_eq!(incomplete.len(), 1);

    store.delete_for_execution("exec-001").unwrap();
    assert_eq!(store.count().unwrap(), 0);
}

#[test]
fn test_checkpoint_from_result() {
    use crate::checkpoint::checkpoint_from_result;

    let result = ExecutionResult {
        execution_id: "exec-001".into(),
        project_id: None,
        status: ExecutionStatus::Completed,
        phase: ExecutionPhase::Validation,
        artifacts: vec![],
        validation: None,
        confidence: 0.95,
        tokens_consumed: 500,
        cpu_seconds_used: 2.0,
        memory_peak_mb: 512.0,
        latency_ms: 1500,
        error_log: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        completed_at: None,
    };

    let ckpt = checkpoint_from_result("exec-001", ExecutionPhase::Validation, &result);
    assert_eq!(ckpt.execution_id, "exec-001");
    assert_eq!(ckpt.phase, ExecutionPhase::Validation);
    assert_eq!(ckpt.confidence, 0.95);
}

#[test]
fn test_audit_severity_enum() {
    use crate::audit::AuditSeverity;
    let _ = AuditSeverity::Critical;
    let _ = AuditSeverity::High;
    let _ = AuditSeverity::Info;
}

#[test]
fn test_sandbox_build_python_command() {
    let work_dir = std::env::temp_dir().join("vee-test-work");
    let output_dir = std::env::temp_dir().join("vee-test-output");
    let _ = std::fs::remove_dir_all(&work_dir);
    let _ = std::fs::remove_dir_all(&output_dir);

    let (_cmd, config) =
        crate::sandbox::build_python_command("print('hello')", &work_dir, &output_dir).unwrap();

    assert!(work_dir.exists());
    assert!(output_dir.exists());
    assert!(work_dir.join("script.py").exists());
    assert!(config.block_network);

    // Cleanup
    let _ = std::fs::remove_dir_all(&work_dir);
    let _ = std::fs::remove_dir_all(&output_dir);
}

#[test]
fn test_real_validation_pass() {
    use crate::validation::validate_artifacts;

    let artifacts = vec![Artifact::Dataset {
        schema: vec![
            ColumnDef {
                name: "date".into(),
                data_type: DataType::DateTime,
                nullable: false,
            },
            ColumnDef {
                name: "value".into(),
                data_type: DataType::Float,
                nullable: true,
            },
        ],
        row_count: 100,
        sample: vec![],
        memory_bytes: 1000,
    }];

    let hypothesis = ExecutionHypothesis {
        expected_columns: vec!["date".into(), "value".into()],
        expected_row_count: Some(Range { min: 50, max: 200 }),
        expected_types: {
            let mut m = HashMap::new();
            m.insert("date".into(), DataType::DateTime);
            m.insert("value".into(), DataType::Float);
            m
        },
        invariants: vec!["no duplicate rows".into()],
        expected_schema: None,
    };

    let result = validate_artifacts(&artifacts, &hypothesis, "", Some(0));
    assert!(result.hypothesis_validated);
    assert!(result.confidence >= 0.9);
    assert!(result.checks.iter().any(|c| c.name == "exit_code_zero"));
    assert!(result.checks.iter().any(|c| c.name == "dataset_produced"));
    assert!(result.checks.iter().any(|c| c.name == "row_count"));
}

#[test]
fn test_real_validation_fail() {
    use crate::validation::validate_artifacts;

    let artifacts = vec![];
    let hypothesis = ExecutionHypothesis {
        expected_columns: vec!["missing".into()],
        expected_row_count: Some(Range { min: 10, max: 20 }),
        expected_types: HashMap::new(),
        invariants: vec![],
        expected_schema: None,
    };

    let result = validate_artifacts(&artifacts, &hypothesis, "error: something broke", Some(1));
    assert!(!result.hypothesis_validated);
    assert!(result.confidence < 0.5);
    assert!(!result.deviations.is_empty());
}

#[test]
fn test_pattern_extraction() {
    use crate::validation::extract_pattern;

    let pattern = extract_pattern(
        "exec-001",
        "import pandas as pd\ndf = pd.read_csv('data.csv')\ndf_sorted = df.sort_values('date')",
        &ExecutionLanguage::Python,
        &ExecutionHypothesis::default(),
        &[],
        1500,
        500,
    );

    assert!(pattern.pattern_id.starts_with("#auto-"));
    assert!(pattern
        .task_signature
        .intent_keywords
        .contains(&"csv".into()));
    assert!(pattern
        .task_signature
        .intent_keywords
        .contains(&"sort".into()));
    assert!(pattern
        .task_signature
        .intent_keywords
        .contains(&"pandas".into()));
    assert!(pattern.tags.contains(&"csv".into()));
    assert!(pattern.tags.contains(&"pandas".into()));
    assert_eq!(pattern.success_rate, 1.0);
}

#[test]
fn test_sandbox_extract_artifacts() {
    use crate::sandbox::SandboxResult;

    let result = SandboxResult {
        stdout: "hello world\nline 2".to_string(),
        stderr: "warning: something\nerror: oops".to_string(),
        exit_code: Some(0),
        duration_ms: 100,
        memory_peak_kb: 1024,
        sandbox_layers_applied: vec!["rlimit".into()],
        sandbox_errors: vec![],
    };

    let output_dir = std::env::temp_dir().join("vee-test-artifacts");
    let _ = std::fs::remove_dir_all(&output_dir);
    std::fs::create_dir_all(&output_dir).unwrap();
    std::fs::write(output_dir.join("test.txt"), "file content").unwrap();

    let artifacts = crate::sandbox::extract_artifacts(&result, &output_dir);

    // Should have: text, log, file, json (meta)
    assert!(artifacts
        .iter()
        .any(|a| matches!(a, crate::types::Artifact::Text { .. })));
    assert!(artifacts
        .iter()
        .any(|a| matches!(a, crate::types::Artifact::Log { .. })));
    assert!(artifacts
        .iter()
        .any(|a| matches!(a, crate::types::Artifact::File { .. })));

    // Cleanup
    let _ = std::fs::remove_dir_all(&output_dir);
}

#[test]
fn test_execution_status_enum_equality() {
    assert_eq!(ExecutionStatus::Completed, ExecutionStatus::Completed);
    assert_ne!(ExecutionStatus::Pending, ExecutionStatus::Completed);
}

#[test]
fn test_execution_language_display_for_integrated_tools() {
    assert_eq!(ExecutionLanguage::Go.to_string(), "go");
    assert_eq!(
        ExecutionLanguage::ContextBundle.to_string(),
        "context_bundle"
    );
}

#[test]
fn test_artifact_summary_from_text() {
    let artifact = Artifact::Text {
        content: "Hello world".into(),
        format: TextFormat::Plain,
        line_count: 1,
    };
    let summary = ArtifactSummary::from(&artifact);
    assert_eq!(summary.artifact_type, "text");
    assert_eq!(summary.size_bytes, 11);
}

#[test]
fn test_artifact_summary_from_dataset() {
    let artifact = Artifact::Dataset {
        schema: vec![ColumnDef {
            name: "id".into(),
            data_type: DataType::Integer,
            nullable: false,
        }],
        row_count: 100,
        sample: vec![],
        memory_bytes: 1000,
    };
    let summary = ArtifactSummary::from(&artifact);
    assert_eq!(summary.artifact_type, "dataset");
}

#[test]
fn test_executor_daemon_try_new_is_fallible_not_panicking() {
    // `try_new` should succeed in a normal environment and not panic.
    let daemon = crate::ExecutorDaemon::try_new();
    assert!(
        daemon.is_ok(),
        "VEE daemon should be constructible: {:?}",
        daemon.err()
    );
}

#[tokio::test]
async fn test_daemon_cancel_queued_execution() {
    let mut registry = crate::capability::CapabilityRegistry::new_with_seed([21u8; 32]);
    let daemon = crate::ExecutorDaemon::try_new_with_verifier(registry.verifier(), None)
        .expect("daemon must construct");
    daemon.start().await;

    let execution_id = "cancel-queued-1";
    let grant = registry.grant(
        execution_id,
        Capability::ProcessSpawn,
        crate::types::GrantAuthority::Orchestrator,
        None,
    );

    let task = ExecutionTask {
        execution_id: execution_id.into(),
        run_id: None,
        agent_id: "test-agent".into(),
        language: ExecutionLanguage::Python,
        source_code: "print('hello')".into(),
        capabilities: vec![Capability::ProcessSpawn],
        capability_grants: vec![grant],
        project_id: None,
        budget: ExecutionBudget {
            cpu_seconds: 1,
            memory_mb: 64,
            disk_mb: 10,
            token_budget: 10,
            wall_clock_seconds: 1,
        },
        hypothesis: None,
        provenance: Provenance::default(),
    };
    daemon.submit(task).await.unwrap();

    daemon.cancel("cancel-queued-1", None).await.unwrap();
    let status = daemon.get_status("cancel-queued-1", None).await.unwrap();
    assert_eq!(status.status, ExecutionStatus::Cancelled);
    daemon.stop().await;
}

// ─────────────────────────────────────────────────────────────────────────────
// Osmosis tests
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_osmosis_diff_text_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(
        crate::artifact::ArtifactStore::try_new(
            &tmp.path().join("vee_artifacts.db"),
            &tmp.path().join("artifacts"),
        )
        .unwrap(),
    );

    let execution_id = "exec-diff-1";
    let provenance = Provenance {
        artifact_id: "art-left".into(),
        task_id: execution_id.into(),
        execution_id: execution_id.into(),
        creator_agent: "test".into(),
        parent_artifacts: vec![],
        code_generator: "test".into(),
        executed_code: "old".into(),
        granted_capabilities: vec![],
        created_at: chrono::Utc::now(),
        previous_hash: "genesis".into(),
        self_hash: String::new(),
    };

    let left = Artifact::Text {
        content: "hello\nworld\n".into(),
        format: TextFormat::Plain,
        line_count: 2,
    };
    let left_id = store.store(left, Some(provenance.clone())).await.unwrap();

    let right = Artifact::Text {
        content: "hello\nViCo\n".into(),
        format: TextFormat::Plain,
        line_count: 2,
    };
    let mut right_prov = provenance.clone();
    right_prov.executed_code = "new".into();
    let right_id = store.store(right, Some(right_prov)).await.unwrap();

    let engine = OsmosisEngine::new(store);
    let result = engine
        .diff(
            None,
            &OsmosisDiffRequest {
                left: OsmosisArtifactRef {
                    execution_id: execution_id.into(),
                    artifact_id: Some(left_id),
                },
                right: Some(OsmosisArtifactRef {
                    execution_id: execution_id.into(),
                    artifact_id: Some(right_id),
                }),
                target_path: None,
                format: Some(OsmosisDiffFormat::Structured),
            },
        )
        .await
        .unwrap();

    assert_eq!(result.structured.files.len(), 1);
    let kinds: Vec<&str> = result.structured.files[0].hunks[0]
        .lines
        .iter()
        .map(|l| l.kind.as_str())
        .collect();
    assert!(kinds.contains(&"remove"));
    assert!(kinds.contains(&"add"));
}

#[tokio::test]
async fn test_osmosis_merge_and_reject() {
    let tmp = tempfile::tempdir().unwrap();
    let project_root = tmp.path().join("project");
    tokio::fs::create_dir_all(&project_root).await.unwrap();

    let store = Arc::new(
        crate::artifact::ArtifactStore::try_new(
            &tmp.path().join("vee_artifacts.db"),
            &tmp.path().join("artifacts"),
        )
        .unwrap(),
    );

    let execution_id = "exec-merge-1";
    let provenance = Provenance {
        artifact_id: "art-patch".into(),
        task_id: execution_id.into(),
        execution_id: execution_id.into(),
        creator_agent: "test".into(),
        parent_artifacts: vec![],
        code_generator: "test".into(),
        executed_code: "patch".into(),
        granted_capabilities: vec![],
        created_at: chrono::Utc::now(),
        previous_hash: "genesis".into(),
        self_hash: String::new(),
    };

    let patch = Artifact::Text {
        content: "proposed content".into(),
        format: TextFormat::Plain,
        line_count: 1,
    };
    let patch_id = store.store(patch, Some(provenance.clone())).await.unwrap();

    let base = Artifact::Text {
        content: "original content".into(),
        format: TextFormat::Plain,
        line_count: 1,
    };
    let mut base_prov = provenance.clone();
    base_prov.executed_code = "base".into();
    let base_id = store.store(base, Some(base_prov)).await.unwrap();

    let engine = OsmosisEngine::new(store.clone());

    let merge_result = engine
        .merge(
            Some(&project_root),
            &OsmosisMergeRequest {
                source: OsmosisArtifactRef {
                    execution_id: execution_id.into(),
                    artifact_id: Some(patch_id.clone()),
                },
                target_path: "output.txt".into(),
                strategy: None,
                base: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(merge_result.bytes_written, "proposed content".len());

    let written = tokio::fs::read_to_string(project_root.join("output.txt"))
        .await
        .unwrap();
    assert_eq!(written, "proposed content");

    let reject_result = engine
        .reject(
            Some(&project_root),
            &OsmosisRejectRequest {
                source: OsmosisArtifactRef {
                    execution_id: execution_id.into(),
                    artifact_id: Some(patch_id.clone()),
                },
                target_path: "output.txt".into(),
                base: Some(OsmosisArtifactRef {
                    execution_id: execution_id.into(),
                    artifact_id: Some(base_id),
                }),
                reason: Some("rolled back".into()),
            },
        )
        .await
        .unwrap();
    assert!(reject_result.restored);

    let restored = tokio::fs::read_to_string(project_root.join("output.txt"))
        .await
        .unwrap();
    assert_eq!(restored, "original content");
}

#[test]
fn test_osmosis_large_diff_performance() {
    // Sanity-check that the LCS diff stays performant for moderately large text.
    let base: Vec<String> = (0..500).map(|i| format!("line {}", i)).collect();
    let mut changed = base.clone();
    changed[100] = "modified line".into();
    changed.push("extra line".into());

    let old_text = base.join("\n");
    let new_text = changed.join("\n");

    let start = std::time::Instant::now();
    let diff = diff_text(&old_text, &new_text);
    let elapsed = start.elapsed();

    assert_eq!(diff.files.len(), 1);
    assert!(!diff.files[0].hunks.is_empty());
    // Should complete in well under a second for 500-line inputs.
    assert!(
        elapsed.as_millis() < 1000,
        "diff took too long: {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_daemon_rejects_capabilities_without_grants() {
    let registry = CapabilityRegistry::new_with_seed([22u8; 32]);
    let daemon = crate::ExecutorDaemon::try_new_with_verifier(registry.verifier(), None)
        .expect("daemon must construct");

    let task = ExecutionTask {
        execution_id: "no-grants-1".into(),
        run_id: None,
        agent_id: "test-agent".into(),
        language: ExecutionLanguage::Python,
        source_code: "print('hello')".into(),
        capabilities: vec![Capability::ProcessSpawn],
        capability_grants: vec![],
        project_id: None,
        budget: ExecutionBudget {
            cpu_seconds: 1,
            memory_mb: 64,
            disk_mb: 10,
            token_budget: 10,
            wall_clock_seconds: 1,
        },
        hypothesis: None,
        provenance: Provenance::default(),
    };

    let result = daemon.submit(task).await;
    assert!(result.is_err(), "expected submit to fail without grants");
    assert!(result
        .unwrap_err()
        .contains("missing or invalid capability grant"));
}

#[tokio::test]
async fn test_worker_rejects_tampered_grant() {
    let mut registry = CapabilityRegistry::new_with_seed([23u8; 32]);
    let execution_id = "tamper-test-1";
    let mut grant = registry.grant(
        execution_id,
        Capability::ProcessSpawn,
        GrantAuthority::Orchestrator,
        None,
    );
    grant.signature.truncate(grant.signature.len() - 1);

    let mut worker = crate::worker::PythonWorker::new();
    let result = worker
        .init(
            execution_id,
            vec![grant],
            Arc::new(registry.verifier()),
            vec![Capability::ProcessSpawn],
            ExecutionBudget::default(),
        )
        .await;

    assert!(result.is_err(), "expected worker to reject tampered grant");
}

#[tokio::test]
async fn test_worker_rejects_wrong_execution_id_grant() {
    let mut registry = CapabilityRegistry::new_with_seed([24u8; 32]);
    let grant = registry.grant(
        "other-execution",
        Capability::ProcessSpawn,
        GrantAuthority::Orchestrator,
        None,
    );

    let mut worker = crate::worker::PythonWorker::new();
    let result = worker
        .init(
            "this-execution",
            vec![grant],
            Arc::new(registry.verifier()),
            vec![Capability::ProcessSpawn],
            ExecutionBudget::default(),
        )
        .await;

    assert!(
        result.is_err(),
        "expected worker to reject grant for wrong execution"
    );
}

#[tokio::test]
async fn test_worker_accepts_valid_grant_and_executes() {
    let mut registry = CapabilityRegistry::new_with_seed([25u8; 32]);
    let execution_id = "valid-grant-1";
    let grant = registry.grant(
        execution_id,
        Capability::ProcessSpawn,
        GrantAuthority::Orchestrator,
        None,
    );

    let mut worker = crate::worker::PythonWorker::new();
    worker
        .init(
            execution_id,
            vec![grant],
            Arc::new(registry.verifier()),
            vec![Capability::ProcessSpawn],
            ExecutionBudget {
                cpu_seconds: 1,
                memory_mb: 64,
                disk_mb: 10,
                token_budget: 10,
                wall_clock_seconds: 5,
            },
        )
        .await
        .expect("worker should accept valid grant");

    let task = ExecutionTask {
        execution_id: execution_id.into(),
        run_id: None,
        agent_id: "test".into(),
        language: ExecutionLanguage::Python,
        source_code: "print('ok')".into(),
        capabilities: vec![Capability::ProcessSpawn],
        capability_grants: vec![],
        project_id: None,
        budget: ExecutionBudget {
            cpu_seconds: 1,
            memory_mb: 64,
            disk_mb: 10,
            token_budget: 10,
            wall_clock_seconds: 5,
        },
        hypothesis: None,
        provenance: Provenance::default(),
    };

    worker
        .execute(&task)
        .await
        .expect("execution should succeed");
}

#[tokio::test]
async fn test_daemon_submit_python_task_completes_with_stdout_artifact() {
    let mut registry = CapabilityRegistry::new_with_seed([41u8; 32]);
    let daemon = crate::ExecutorDaemon::try_new_with_verifier(registry.verifier(), None)
        .expect("daemon must construct");
    daemon.start().await;

    let execution_id = "python-stdout-1";
    let grant = registry.grant(
        execution_id,
        Capability::ProcessSpawn,
        GrantAuthority::Orchestrator,
        None,
    );

    let task = ExecutionTask {
        execution_id: execution_id.into(),
        run_id: None,
        agent_id: "test-agent".into(),
        language: ExecutionLanguage::Python,
        source_code: "print('hello from vee')".into(),
        capabilities: vec![Capability::ProcessSpawn],
        capability_grants: vec![grant],
        project_id: None,
        budget: ExecutionBudget {
            cpu_seconds: 5,
            memory_mb: 128,
            disk_mb: 10,
            token_budget: 10,
            wall_clock_seconds: 10,
        },
        hypothesis: None,
        provenance: Provenance::default(),
    };

    daemon.submit(task).await.expect("submit should succeed");

    let mut final_result = None;
    for _ in 0..50 {
        if let Some(r) = daemon.get_status(execution_id, None).await {
            if matches!(
                r.status,
                ExecutionStatus::Completed | ExecutionStatus::Failed | ExecutionStatus::Cancelled
            ) {
                final_result = Some(r);
                break;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    let result = final_result.expect("execution should reach a terminal state");
    assert_eq!(
        result.status,
        ExecutionStatus::Completed,
        "expected completed: {:?}",
        result.error_log
    );
    assert!(result.latency_ms > 0, "latency should be recorded");

    let stdout = result.artifacts.iter().find_map(|a| match a {
        Artifact::Text { content, .. } => Some(content.as_str()),
        _ => None,
    });
    assert!(
        stdout.unwrap_or("").contains("hello from vee"),
        "stdout artifact missing expected content: {:?}",
        stdout
    );

    daemon.stop().await;
}

#[tokio::test]
async fn test_daemon_submit_then_cancel_marks_cancelled() {
    let mut registry = CapabilityRegistry::new_with_seed([42u8; 32]);
    let daemon = crate::ExecutorDaemon::try_new_with_verifier(registry.verifier(), None)
        .expect("daemon must construct");
    daemon.start().await;

    let execution_id = "cancel-inflight-1";
    let grant = registry.grant(
        execution_id,
        Capability::ProcessSpawn,
        GrantAuthority::Orchestrator,
        None,
    );

    let task = ExecutionTask {
        execution_id: execution_id.into(),
        run_id: None,
        agent_id: "test-agent".into(),
        language: ExecutionLanguage::Python,
        source_code: "import time; time.sleep(10)".into(),
        capabilities: vec![Capability::ProcessSpawn],
        capability_grants: vec![grant],
        project_id: None,
        budget: ExecutionBudget {
            cpu_seconds: 15,
            memory_mb: 128,
            disk_mb: 10,
            token_budget: 10,
            wall_clock_seconds: 15,
        },
        hypothesis: None,
        provenance: Provenance::default(),
    };

    daemon.submit(task).await.expect("submit should succeed");
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    daemon
        .cancel(execution_id, None)
        .await
        .expect("cancel should succeed");

    let status = daemon
        .get_status(execution_id, None)
        .await
        .expect("result should exist");
    assert_eq!(status.status, ExecutionStatus::Cancelled);

    daemon.stop().await;
}

#[tokio::test]
async fn test_daemon_submit_without_grant_rejects() {
    let registry = CapabilityRegistry::new_with_seed([43u8; 32]);
    let daemon = crate::ExecutorDaemon::try_new_with_verifier(registry.verifier(), None)
        .expect("daemon must construct");

    let task = ExecutionTask {
        execution_id: "missing-grant-1".into(),
        run_id: None,
        agent_id: "test-agent".into(),
        language: ExecutionLanguage::Python,
        source_code: "print('hello')".into(),
        capabilities: vec![Capability::ProcessSpawn],
        capability_grants: vec![],
        project_id: None,
        budget: ExecutionBudget::default(),
        hypothesis: None,
        provenance: Provenance::default(),
    };

    let result = daemon.submit(task).await;
    assert!(result.is_err(), "expected submit to fail without grants");
    assert!(
        result
            .unwrap_err()
            .contains("missing or invalid capability grant"),
        "error should mention missing grant"
    );
}
