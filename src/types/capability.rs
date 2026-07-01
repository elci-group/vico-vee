use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Capabilities
// ─────────────────────────────────────────────────────────────────────────────

/// A fine-grained permission to access a resource or perform an action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "params")]
pub enum Capability {
    /// Read from specified filesystem paths
    FilesystemRead { paths: Vec<String> },
    /// Write to specified filesystem paths
    FilesystemWrite { paths: Vec<String> },
    /// Create files in specified paths
    FilesystemCreate { paths: Vec<String> },
    /// Network access to specified hosts/ports
    NetworkAccess { hosts: Vec<String>, ports: Vec<u16> },
    /// DNS resolution
    NetworkDns,
    /// CPU core allocation
    CpuCores(u8),
    /// Memory allocation in MB
    MemoryMB(u64),
    /// GPU compute access
    GpuCompute { device: Option<u8> },
    /// Spawn child processes
    ProcessSpawn,
    /// Read environment variables
    EnvironmentRead,
    /// Write environment variables
    EnvironmentWrite,
    /// Database access
    DatabaseAccess { connection_string: String },
    /// Browser automation
    BrowserAutomation,
    /// Audio capture
    AudioCapture,
    /// Video capture
    VideoCapture,
    /// Read from SSME
    SsmeRead,
    /// Write to SSME
    SsmeWrite,
    /// Query belief graph
    BeliefGraphQuery,
    /// Use specific inference provider
    InferenceProvider { provider: String },
}

impl Capability {
    /// Human-readable name for this capability type.
    pub fn name(&self) -> &'static str {
        match self {
            Capability::FilesystemRead { .. } => "filesystem_read",
            Capability::FilesystemWrite { .. } => "filesystem_write",
            Capability::FilesystemCreate { .. } => "filesystem_create",
            Capability::NetworkAccess { .. } => "network_access",
            Capability::NetworkDns => "network_dns",
            Capability::CpuCores(_) => "cpu_cores",
            Capability::MemoryMB(_) => "memory_mb",
            Capability::GpuCompute { .. } => "gpu_compute",
            Capability::ProcessSpawn => "process_spawn",
            Capability::EnvironmentRead => "environment_read",
            Capability::EnvironmentWrite => "environment_write",
            Capability::DatabaseAccess { .. } => "database_access",
            Capability::BrowserAutomation => "browser_automation",
            Capability::AudioCapture => "audio_capture",
            Capability::VideoCapture => "video_capture",
            Capability::SsmeRead => "ssme_read",
            Capability::SsmeWrite => "ssme_write",
            Capability::BeliefGraphQuery => "belief_graph_query",
            Capability::InferenceProvider { .. } => "inference_provider",
        }
    }
}

/// Authority that issued a capability grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GrantAuthority {
    Orchestrator,
    Afa,
    Human,
    System,
}

/// A signed capability grant bound to a specific execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub execution_id: String,
    pub capability: Capability,
    pub granted: bool,
    pub granted_by: GrantAuthority,
    pub reason: Option<String>,
    pub timestamp: DateTime<Utc>,
    /// Ed25519-signed JWT capability token.
    pub signature: String,
}
