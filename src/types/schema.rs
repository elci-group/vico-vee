use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Execution Hypothesis & Validation
// ─────────────────────────────────────────────────────────────────────────────

/// Before execution: what does the agent expect?
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionHypothesis {
    /// Expected output schema
    pub expected_schema: Option<DataSchema>,
    /// Expected row count range
    pub expected_row_count: Option<Range<usize>>,
    /// Expected column names
    pub expected_columns: Vec<String>,
    /// Expected data types per column
    pub expected_types: HashMap<String, DataType>,
    /// Invariants that must hold (e.g., "date column is sorted ascending")
    pub invariants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataSchema {
    pub columns: Vec<ColumnDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataType {
    String,
    Integer,
    Float,
    Boolean,
    DateTime,
    Json,
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range<T> {
    pub min: T,
    pub max: T,
}

/// After execution: did reality match hypothesis?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Did the hypothesis hold?
    pub hypothesis_validated: bool,
    /// Specific check results
    pub checks: Vec<ValidationCheck>,
    /// Confidence score (0.0-1.0)
    pub confidence: f64,
    /// Deviations from hypothesis (if any)
    pub deviations: Vec<Deviation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deviation {
    pub field: String,
    pub expected: String,
    pub actual: String,
    pub severity: DeviationSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviationSeverity {
    Minor,
    Major,
    Critical,
}
