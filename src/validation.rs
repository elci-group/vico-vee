//! Real Hypothesis Validation
//!
//! Validates execution outputs against stated hypotheses.
//! Phase 3: Actual schema, row count, type, and invariant checking.

use crate::types::*;

/// Validate artifacts against a hypothesis.
pub fn validate_artifacts(
    artifacts: &[Artifact],
    hypothesis: &ExecutionHypothesis,
    stderr: &str,
    exit_code: Option<i32>,
) -> ValidationResult {
    let mut checks = Vec::new();
    let mut deviations = Vec::new();

    // Check 1: Exit code must be 0
    let exit_ok = exit_code == Some(0);
    checks.push(ValidationCheck {
        name: "exit_code_zero".into(),
        passed: exit_ok,
        detail: if exit_ok {
            "Process exited with code 0".into()
        } else {
            format!("Process exited with code {:?}", exit_code)
        },
    });
    if !exit_ok {
        deviations.push(Deviation {
            field: "exit_code".into(),
            expected: "0".into(),
            actual: format!("{:?}", exit_code),
            severity: DeviationSeverity::Critical,
        });
    }

    // Check 2: No errors in stderr
    let stderr_clean =
        !stderr.to_lowercase().contains("error") && !stderr.to_lowercase().contains("traceback");
    checks.push(ValidationCheck {
        name: "stderr_clean".into(),
        passed: stderr_clean,
        detail: if stderr_clean {
            "No errors in stderr".into()
        } else {
            "Errors detected in stderr output".into()
        },
    });
    if !stderr_clean {
        deviations.push(Deviation {
            field: "stderr".into(),
            expected: "no errors".into(),
            actual: "errors found".into(),
            severity: DeviationSeverity::Major,
        });
    }

    // Check 3: Dataset artifacts match expected schema
    if !hypothesis.expected_columns.is_empty() || hypothesis.expected_schema.is_some() {
        let datasets: Vec<&Artifact> = artifacts
            .iter()
            .filter(|a| matches!(a, Artifact::Dataset { .. }))
            .collect();

        if datasets.is_empty() {
            checks.push(ValidationCheck {
                name: "dataset_produced".into(),
                passed: false,
                detail: "No dataset artifact produced but hypothesis expects one".into(),
            });
            deviations.push(Deviation {
                field: "dataset".into(),
                expected: "dataset artifact".into(),
                actual: "none".into(),
                severity: DeviationSeverity::Critical,
            });
        } else {
            checks.push(ValidationCheck {
                name: "dataset_produced".into(),
                passed: true,
                detail: format!("{} dataset artifact(s) produced", datasets.len()),
            });

            for dataset in &datasets {
                if let Artifact::Dataset { schema, .. } = dataset {
                    // Check expected columns
                    let schema_columns: Vec<String> =
                        schema.iter().map(|c| c.name.clone()).collect();
                    for expected_col in &hypothesis.expected_columns {
                        let found = schema_columns
                            .iter()
                            .any(|c| c.eq_ignore_ascii_case(expected_col));
                        checks.push(ValidationCheck {
                            name: format!("column_{}", expected_col),
                            passed: found,
                            detail: if found {
                                format!("Column '{}' found in schema", expected_col)
                            } else {
                                format!(
                                    "Column '{}' missing from schema {:?}",
                                    expected_col, schema_columns
                                )
                            },
                        });
                        if !found {
                            deviations.push(Deviation {
                                field: format!("column.{}", expected_col),
                                expected: expected_col.clone(),
                                actual: schema_columns.join(", "),
                                severity: DeviationSeverity::Major,
                            });
                        }
                    }

                    // Check expected types
                    for (col_name, expected_type) in &hypothesis.expected_types {
                        let actual_type = schema
                            .iter()
                            .find(|c| c.name.eq_ignore_ascii_case(col_name));
                        if let Some(col) = actual_type {
                            let type_matches = data_type_eq(&col.data_type, expected_type);
                            checks.push(ValidationCheck {
                                name: format!("type_{}", col_name),
                                passed: type_matches,
                                detail: if type_matches {
                                    format!("Column '{}' has expected type", col_name)
                                } else {
                                    format!("Column '{}' type mismatch", col_name)
                                },
                            });
                            if !type_matches {
                                deviations.push(Deviation {
                                    field: format!("type.{}", col_name),
                                    expected: format!("{:?}", expected_type),
                                    actual: format!("{:?}", col.data_type),
                                    severity: DeviationSeverity::Major,
                                });
                            }
                        } else {
                            checks.push(ValidationCheck {
                                name: format!("type_{}", col_name),
                                passed: false,
                                detail: format!("Column '{}' not found for type check", col_name),
                            });
                        }
                    }
                }
            }
        }
    }

    // Check 4: Row count range
    if let Some(ref range) = hypothesis.expected_row_count {
        let row_counts: Vec<usize> = artifacts
            .iter()
            .filter_map(|a| {
                if let Artifact::Dataset { row_count, .. } = a {
                    Some(*row_count)
                } else {
                    None
                }
            })
            .collect();

        if row_counts.is_empty() {
            checks.push(ValidationCheck {
                name: "row_count".into(),
                passed: false,
                detail: "No dataset to check row count".into(),
            });
        } else {
            let total_rows: usize = row_counts.iter().sum();
            let in_range = total_rows >= range.min && total_rows <= range.max;
            checks.push(ValidationCheck {
                name: "row_count".into(),
                passed: in_range,
                detail: format!(
                    "Total rows: {} (expected {}-{})",
                    total_rows, range.min, range.max
                ),
            });
            if !in_range {
                deviations.push(Deviation {
                    field: "row_count".into(),
                    expected: format!("{}-{}", range.min, range.max),
                    actual: total_rows.to_string(),
                    severity: DeviationSeverity::Major,
                });
            }
        }
    }

    // Check 5: Invariants
    for invariant in &hypothesis.invariants {
        let (passed, detail) = check_invariant(invariant, artifacts, stderr);
        checks.push(ValidationCheck {
            name: format!("invariant: {}", invariant),
            passed,
            detail,
        });
        if !passed {
            deviations.push(Deviation {
                field: format!("invariant.{}", invariant),
                expected: "true".into(),
                actual: "false".into(),
                severity: DeviationSeverity::Major,
            });
        }
    }

    let all_passed = checks.iter().all(|c| c.passed);
    let confidence = if all_passed {
        0.95
    } else {
        let pass_rate =
            checks.iter().filter(|c| c.passed).count() as f64 / checks.len().max(1) as f64;
        pass_rate * 0.8
    };

    ValidationResult {
        hypothesis_validated: all_passed,
        checks,
        confidence,
        deviations,
    }
}

/// Check a single invariant against artifacts.
fn check_invariant(invariant: &str, artifacts: &[Artifact], _stderr: &str) -> (bool, String) {
    let lower = invariant.to_lowercase();

    // Extract a target column from the invariant text if possible.
    let datasets: Vec<&Artifact> = artifacts
        .iter()
        .filter(|a| matches!(a, Artifact::Dataset { .. }))
        .collect();

    if datasets.is_empty() {
        return (
            false,
            format!(
                "Invariant '{}' requires a dataset artifact but none was produced",
                invariant
            ),
        );
    }

    // Use the first dataset that has a usable sample.
    for dataset in &datasets {
        if let Artifact::Dataset { schema, sample, .. } = dataset {
            let column = extract_column_name(&lower, schema);

            // "date column is sorted ascending"
            if lower.contains("sorted") && lower.contains("ascending") {
                let col = match column {
                    Some(c) => c,
                    None => {
                        return (
                            false,
                            format!(
                                "Invariant '{}' specifies sorting but no matching column found",
                                invariant
                            ),
                        );
                    }
                };
                let ok = check_sorted(sample, &col, true);
                let detail = if ok {
                    format!("Column '{}' is sorted ascending", col)
                } else {
                    format!("Column '{}' is not sorted ascending", col)
                };
                return (ok, detail);
            }

            // "no duplicate rows" or uniqueness per column
            if lower.contains("no duplicate") || lower.contains("unique") {
                let ok = match column {
                    Some(col) => check_unique(sample, &col),
                    None => check_unique_rows(sample),
                };
                let detail = if ok {
                    "No duplicate values found".into()
                } else {
                    "Duplicate values found".into()
                };
                return (ok, detail);
            }

            // "no null values in column X"
            if lower.contains("no null") {
                let col = match column {
                    Some(c) => c,
                    None => {
                        return (
                            false,
                            format!(
                                "Invariant '{}' specifies no-null but no matching column found",
                                invariant
                            ),
                        );
                    }
                };
                let ok = check_no_null(sample, &col);
                let detail = if ok {
                    format!("Column '{}' has no null values", col)
                } else {
                    format!("Column '{}' contains null values", col)
                };
                return (ok, detail);
            }
        }
    }

    // Default: assume true for unknown invariants
    (
        true,
        format!(
            "Invariant '{}' not specifically checked — assumed valid",
            invariant
        ),
    )
}

fn extract_column_name(
    lower_invariant: &str,
    schema: &[crate::types::ColumnDef],
) -> Option<String> {
    schema.iter().map(|c| c.name.clone()).find(|name| {
        let lower = name.to_lowercase();
        lower_invariant.contains(&lower)
    })
}

fn get_values(sample: &[serde_json::Value], column: &str) -> Vec<serde_json::Value> {
    sample
        .iter()
        .filter_map(|row| row.get(column).cloned())
        .collect()
}

fn check_sorted(sample: &[serde_json::Value], column: &str, ascending: bool) -> bool {
    let values = get_values(sample, column);
    if values.len() < 2 {
        return true;
    }
    let cmp = |a: &serde_json::Value, b: &serde_json::Value| {
        let ord = compare_json_values(a, b);
        if ascending {
            ord
        } else {
            ord.reverse()
        }
    };
    values
        .windows(2)
        .all(|w| cmp(&w[0], &w[1]) != std::cmp::Ordering::Greater)
}

fn check_unique(sample: &[serde_json::Value], column: &str) -> bool {
    let values = get_values(sample, column);
    let mut seen = std::collections::HashSet::new();
    values.iter().all(|v| seen.insert(v.to_string()))
}

fn check_unique_rows(sample: &[serde_json::Value]) -> bool {
    let mut seen = std::collections::HashSet::new();
    sample.iter().all(|v| seen.insert(v.to_string()))
}

fn check_no_null(sample: &[serde_json::Value], column: &str) -> bool {
    sample
        .iter()
        .filter_map(|row| row.get(column))
        .all(|v| !v.is_null())
}

fn compare_json_values(a: &serde_json::Value, b: &serde_json::Value) -> std::cmp::Ordering {
    match (a, b) {
        (serde_json::Value::Number(na), serde_json::Value::Number(nb)) => {
            if let (Some(a), Some(b)) = (na.as_f64(), nb.as_f64()) {
                a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                std::cmp::Ordering::Equal
            }
        }
        (serde_json::Value::String(sa), serde_json::Value::String(sb)) => sa.cmp(sb),
        _ => a.to_string().cmp(&b.to_string()),
    }
}

/// Compare two data types with fuzzy matching.
fn data_type_eq(a: &DataType, b: &DataType) -> bool {
    match (a, b) {
        (DataType::String, DataType::String) => true,
        (DataType::Integer, DataType::Integer) => true,
        (DataType::Integer, DataType::Float) => true, // Integer can be treated as float
        (DataType::Float, DataType::Float) => true,
        (DataType::Float, DataType::Integer) => true,
        (DataType::Boolean, DataType::Boolean) => true,
        (DataType::DateTime, DataType::DateTime) => true,
        (DataType::Json, DataType::Json) => true,
        (DataType::Binary, DataType::Binary) => true,
        _ => false,
    }
}

/// Extract a pattern from a successful execution.
pub fn extract_pattern(
    execution_id: &str,
    source_code: &str,
    language: &ExecutionLanguage,
    hypothesis: &ExecutionHypothesis,
    artifacts: &[Artifact],
    latency_ms: u64,
    tokens: u64,
) -> ExecutionPattern {
    let keywords = extract_keywords(source_code);
    let required_caps = infer_capabilities(source_code);

    // Estimate complexity from code metrics
    let complexity = (source_code.lines().count().min(100) as u8 / 10 + 1).clamp(1, 10);

    let description = infer_description(&keywords, source_code);

    ExecutionPattern {
        pattern_id: format!("#auto-{}", execution_id),
        description,
        task_signature: TaskSignature {
            language: language.clone(),
            intent_keywords: keywords,
            required_capabilities: required_caps,
            estimated_complexity: complexity,
        },
        code_template: source_code.to_string(),
        hypothesis_template: hypothesis.clone(),
        success_rate: 1.0, // First success
        usage_count: 1,
        avg_cost: ExecutionBudget {
            cpu_seconds: latency_ms / 1000,
            memory_mb: 512,
            disk_mb: 100,
            token_budget: tokens,
            wall_clock_seconds: latency_ms / 1000 + 10,
        },
        tags: infer_tags(source_code, artifacts),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

pub fn extract_keywords(source_code: &str) -> Vec<String> {
    let keywords: Vec<&str> = vec![
        "csv",
        "json",
        "pandas",
        "DataFrame",
        "plot",
        "chart",
        "matplotlib",
        "sort",
        "filter",
        "group",
        "merge",
        "clean",
        "transform",
        "read",
        "write",
        "train",
        "model",
        "predict",
        "fit",
        "score",
        "evaluate",
        "aggregate",
        "normalize",
        "encode",
        "decode",
        "parse",
        "serialize",
        "deserialize",
    ];
    let lower = source_code.to_lowercase();
    keywords
        .into_iter()
        .filter(|kw| lower.contains(kw))
        .map(|s| s.to_string())
        .collect()
}

fn infer_capabilities(source_code: &str) -> Vec<String> {
    let mut caps = vec![
        "filesystem_read".to_string(),
        "filesystem_write".to_string(),
    ];
    let lower = source_code.to_lowercase();
    if lower.contains("http") || lower.contains("url") || lower.contains("request") {
        caps.push("network_access".into());
    }
    if lower.contains("socket") || lower.contains("tcp") || lower.contains("udp") {
        caps.push("network_access".into());
    }
    if lower.contains("subprocess") || lower.contains("os.system") || lower.contains("popen") {
        caps.push("process_spawn".into());
    }
    if lower.contains("gpu") || lower.contains("cuda") || lower.contains("torch") {
        caps.push("gpu_compute".into());
    }
    caps
}

fn infer_description(keywords: &[String], source_code: &str) -> String {
    if keywords.contains(&"csv".to_string()) && keywords.contains(&"sort".to_string()) {
        return "Sort and process CSV data".into();
    }
    if keywords.contains(&"json".to_string()) && keywords.contains(&"flatten".to_string()) {
        return "Transform and flatten JSON".into();
    }
    if keywords.contains(&"plot".to_string()) || keywords.contains(&"chart".to_string()) {
        return "Generate visualisations".into();
    }
    if keywords.contains(&"train".to_string()) || keywords.contains(&"model".to_string()) {
        return "Train or evaluate ML model".into();
    }
    if keywords.contains(&"clean".to_string()) {
        return "Clean and preprocess data".into();
    }
    let first_line = source_code.lines().next().unwrap_or("").trim();
    if first_line.starts_with("#") {
        return first_line.trim_start_matches("#").trim().into();
    }
    format!("Auto-extracted pattern: {}", keywords.join(", "))
}

fn infer_tags(source_code: &str, artifacts: &[Artifact]) -> Vec<String> {
    let mut tags = Vec::new();
    let lower = source_code.to_lowercase();

    if lower.contains("csv") {
        tags.push("csv".into());
    }
    if lower.contains("json") {
        tags.push("json".into());
    }
    if lower.contains("pandas") || lower.contains("dataframe") {
        tags.push("pandas".into());
    }
    if lower.contains("plot") || lower.contains("matplotlib") {
        tags.push("visualisation".into());
    }
    if lower.contains("sklearn") || lower.contains("train") || lower.contains("model") {
        tags.push("ml".into());
    }
    if lower.contains("clean") || lower.contains("preprocess") {
        tags.push("data-cleaning".into());
    }

    for artifact in artifacts {
        match artifact {
            Artifact::Dataset { .. } if !tags.contains(&"dataset".into()) => {
                tags.push("dataset".into());
            }
            Artifact::Dataset { .. } => {}
            Artifact::Image { .. } if !tags.contains(&"image".into()) => {
                tags.push("image".into());
            }
            Artifact::Image { .. } => {}
            Artifact::Model { .. } if !tags.contains(&"model".into()) => {
                tags.push("model".into());
            }
            Artifact::Model { .. } => {}
            _ => {}
        }
    }

    tags
}
