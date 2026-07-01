use crate::types::*;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::ExecutorDaemon;

/// A minimal audit report produced by the executor daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    pub overall_pass: bool,
    pub passed_count: usize,
    pub failed_count: usize,
    pub critical_failures: usize,
    pub timestamp: String,
    pub tests: Vec<AuditTest>,
}

/// A single audit test entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditTest {
    pub test_name: String,
    pub passed: bool,
    pub severity: String,
    pub detail: String,
}

impl ExecutorDaemon {
    /// Run an audit of the daemon's stored execution results.
    pub fn run_audit(&self) -> AuditReport {
        let store = self.inner.store.blocking_read();
        let mut passed_count = 0usize;
        let mut failed_count = 0usize;
        let mut critical_failures = 0usize;
        let mut tests = Vec::new();

        for result in store.values() {
            match result.status {
                ExecutionStatus::Completed => {
                    passed_count += 1;
                    tests.push(AuditTest {
                        test_name: format!("execution_{}_completed", result.execution_id),
                        passed: true,
                        severity: "info".to_string(),
                        detail: format!(
                            "execution completed in {} ms with {} artifacts",
                            result.latency_ms,
                            result.artifacts.len()
                        ),
                    });
                }
                ExecutionStatus::Failed => {
                    failed_count += 1;
                    critical_failures += 1;
                    tests.push(AuditTest {
                        test_name: format!("execution_{}_failed", result.execution_id),
                        passed: false,
                        severity: "critical".to_string(),
                        detail: result.error_log.clone().unwrap_or_default(),
                    });
                }
                _ => {}
            }
        }

        AuditReport {
            overall_pass: failed_count == 0,
            passed_count,
            failed_count,
            critical_failures,
            timestamp: Utc::now().to_rfc3339(),
            tests,
        }
    }
}
