//! Execution Memory (Patterns)
//!
//! Stores and retrieves successful execution patterns for reuse.

use crate::types::*;
use std::collections::HashMap;

/// Pattern database backed by an in-memory cache with optional SQLite persistence.
pub struct PatternStore {
    patterns: HashMap<String, ExecutionPattern>,
    db: Option<rusqlite::Connection>,
}

impl PatternStore {
    pub fn new() -> Self {
        let mut store = Self {
            patterns: HashMap::new(),
            db: None,
        };
        store.seed_builtin_patterns();
        store
    }

    /// Open (or create) a persistent pattern store at the given path.
    pub fn new_with_path(path: &std::path::Path) -> Result<Self, String> {
        let mut store = Self {
            patterns: HashMap::new(),
            db: None,
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let db = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
        crate::migrations::run_migrations(&db, crate::migrations::MIGRATIONS)
            .map_err(|e| format!("run pattern schema migrations: {e}"))?;

        {
            let mut stmt = db
                .prepare("SELECT data FROM patterns")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |row| {
                    let data: String = row.get(0)?;
                    Ok(data)
                })
                .map_err(|e| e.to_string())?;

            for row in rows {
                let data = row.map_err(|e| e.to_string())?;
                if let Ok(pattern) = serde_json::from_str::<ExecutionPattern>(&data) {
                    store.patterns.insert(pattern.pattern_id.clone(), pattern);
                }
            }
            drop(stmt);
        }

        store.db = Some(db);

        if store.patterns.is_empty() {
            store.seed_builtin_patterns();
        }

        Ok(store)
    }

    /// Store a new pattern (or update an existing one).
    pub fn store(&mut self, pattern: ExecutionPattern) {
        self.patterns
            .insert(pattern.pattern_id.clone(), pattern.clone());
        if let Some(db) = &self.db {
            let data = serde_json::to_string(&pattern).unwrap_or_default();
            let _ = db.execute(
                "INSERT OR REPLACE INTO patterns (pattern_id, data) VALUES (?1, ?2)",
                rusqlite::params![&pattern.pattern_id, &data],
            );
        }
    }

    /// Retrieve a pattern by ID.
    pub fn get(&self, pattern_id: &str) -> Option<&ExecutionPattern> {
        self.patterns.get(pattern_id)
    }

    /// Find patterns matching a task signature, sorted by success rate.
    pub fn find_matches(
        &self,
        signature: &TaskSignature,
        min_similarity: f64,
    ) -> Vec<&ExecutionPattern> {
        let mut matches: Vec<&ExecutionPattern> = self
            .patterns
            .values()
            .filter(|p| {
                p.task_signature.language == signature.language
                    && similarity(
                        &p.task_signature.intent_keywords,
                        &signature.intent_keywords,
                    ) >= min_similarity
            })
            .collect();
        matches.sort_by(|a, b| b.success_rate.total_cmp(&a.success_rate));
        matches
    }

    /// Record a successful execution against a pattern.
    pub fn record_success(&mut self, pattern_id: &str, latency_ms: u64, tokens: u64) {
        if let Some(pattern) = self.patterns.get_mut(pattern_id) {
            pattern.usage_count += 1;
            // Update rolling average success rate
            let n = pattern.usage_count as f64;
            pattern.success_rate = ((pattern.success_rate * (n - 1.0)) + 1.0) / n;
            pattern.updated_at = chrono::Utc::now();
            // Simplified cost averaging
            pattern.avg_cost.cpu_seconds = latency_ms / 1000;
            pattern.avg_cost.token_budget = tokens;

            if let Some(db) = &self.db {
                let data = serde_json::to_string(&pattern).unwrap_or_default();
                let _ = db.execute(
                    "INSERT OR REPLACE INTO patterns (pattern_id, data) VALUES (?1, ?2)",
                    rusqlite::params![pattern_id, &data],
                );
            }
        }
    }

    /// Record a failed execution against a pattern.
    pub fn record_failure(&mut self, pattern_id: &str) {
        if let Some(pattern) = self.patterns.get_mut(pattern_id) {
            pattern.usage_count += 1;
            let n = pattern.usage_count as f64;
            pattern.success_rate = (pattern.success_rate * (n - 1.0)) / n;
            pattern.updated_at = chrono::Utc::now();

            if let Some(db) = &self.db {
                let data = serde_json::to_string(&pattern).unwrap_or_default();
                let _ = db.execute(
                    "INSERT OR REPLACE INTO patterns (pattern_id, data) VALUES (?1, ?2)",
                    rusqlite::params![pattern_id, &data],
                );
            }
        }
    }

    /// List all patterns, optionally filtered by tag.
    pub fn list(&self, tag: Option<&str>) -> Vec<&ExecutionPattern> {
        let mut all: Vec<&ExecutionPattern> = self.patterns.values().collect();
        if let Some(t) = tag {
            all.retain(|p| p.tags.iter().any(|tag| tag.eq_ignore_ascii_case(t)));
        }
        all.sort_by(|a, b| b.success_rate.total_cmp(&a.success_rate));
        all
    }

    fn seed_builtin_patterns(&mut self) {
        let now = chrono::Utc::now();

        // Pattern #1044: CSV Cleaning
        self.store(ExecutionPattern {
            pattern_id: "#1044".to_string(),
            description: "Clean and sort a CSV dataset".to_string(),
            task_signature: TaskSignature {
                language: ExecutionLanguage::Python,
                intent_keywords: vec![
                    "csv".into(),
                    "clean".into(),
                    "sort".into(),
                    "dataset".into(),
                ],
                required_capabilities: vec!["filesystem_read".into(), "filesystem_write".into()],
                estimated_complexity: 3,
            },
            code_template: r#"import pandas as pd
df = pd.read_csv("{{input_path}}")
# Clean: drop NaN, dedupe, sort
df = df.dropna().drop_duplicates().sort_values("{{sort_column}}")
df.to_csv("{{output_path}}", index=False)
"#
            .to_string(),
            hypothesis_template: ExecutionHypothesis {
                expected_columns: vec![],
                expected_row_count: None,
                expected_types: HashMap::new(),
                invariants: vec![
                    "No duplicate rows".into(),
                    "Sorted by specified column".into(),
                ],
                expected_schema: None,
            },
            success_rate: 0.95,
            usage_count: 0,
            avg_cost: ExecutionBudget {
                cpu_seconds: 5,
                memory_mb: 256,
                disk_mb: 50,
                token_budget: 500,
                wall_clock_seconds: 10,
            },
            tags: vec!["data-cleaning".into(), "csv".into(), "python".into()],
            created_at: now,
            updated_at: now,
        });

        // Pattern #1045: JSON Transformation
        self.store(ExecutionPattern {
            pattern_id: "#1045".to_string(),
            description: "Transform and flatten nested JSON".to_string(),
            task_signature: TaskSignature {
                language: ExecutionLanguage::Python,
                intent_keywords: vec!["json".into(), "flatten".into(), "transform".into()],
                required_capabilities: vec!["filesystem_read".into(), "filesystem_write".into()],
                estimated_complexity: 4,
            },
            code_template: r#"import json
with open("{{input_path}}") as f:
    data = json.load(f)
# Flatten nested structure
flat = {}
def flatten(d, prefix=""):
    for k, v in d.items() if isinstance(d, dict) else enumerate(d):
        key = f"{prefix}.{k}" if prefix else str(k)
        if isinstance(v, (dict, list)):
            flatten(v, key)
        else:
            flat[key] = v
flatten(data)
with open("{{output_path}}", "w") as f:
    json.dump(flat, f, indent=2)
"#
            .to_string(),
            hypothesis_template: ExecutionHypothesis {
                expected_columns: vec![],
                expected_row_count: None,
                expected_types: HashMap::new(),
                invariants: vec!["All nested keys are flattened".into()],
                expected_schema: None,
            },
            success_rate: 0.88,
            usage_count: 0,
            avg_cost: ExecutionBudget {
                cpu_seconds: 3,
                memory_mb: 128,
                disk_mb: 20,
                token_budget: 300,
                wall_clock_seconds: 5,
            },
            tags: vec!["json".into(), "transformation".into(), "python".into()],
            created_at: now,
            updated_at: now,
        });
    }
}

impl Default for PatternStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple Jaccard similarity between keyword sets.
fn similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let set_a: std::collections::HashSet<_> = a.iter().map(|s| s.to_lowercase()).collect();
    let set_b: std::collections::HashSet<_> = b.iter().map(|s| s.to_lowercase()).collect();
    let intersection: std::collections::HashSet<_> = set_a.intersection(&set_b).collect();
    let union: std::collections::HashSet<_> = set_a.union(&set_b).collect();
    intersection.len() as f64 / union.len() as f64
}
