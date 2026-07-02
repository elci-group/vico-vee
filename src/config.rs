//! Layered configuration for the standalone `vico-vee` service.
//!
//! Configuration is resolved with the following precedence (lowest to highest):
//!
//! 1. Built-in defaults
//! 2. Config file (`config.toml` or `config.yaml` in `VICO_VEE_CONFIG_DIR`)
//! 3. Environment variables prefixed with `VICO_VEE_`
//! 4. Command-line arguments
//!
//! The top-level [`Config`] struct is produced by [`Config::load`], which takes
//! an optional CLI override set. Callers that do not parse CLI args can use
//! [`Config::default`] or [`Config::from_env`].

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Service configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// TCP port to listen on.
    #[serde(default = "default_port")]
    pub port: u16,

    /// Interface address to bind to.
    #[serde(default = "default_bind")]
    pub bind: String,

    /// Base directory for persistent data.
    #[serde(default = "vee_data_dir")]
    pub data_dir: PathBuf,

    /// Base directory for configuration files.
    #[serde(default = "vee_config_dir")]
    pub config_dir: PathBuf,

    /// URL of the local Ollama instance used by ODIN probes.
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,

    /// Log output format.
    #[serde(default)]
    pub log_format: LogFormat,

    /// Optional path to a TLS certificate (PEM). When both `tls_cert` and
    /// `tls_key` are set the service serves HTTPS.
    pub tls_cert: Option<PathBuf>,

    /// Optional path to a TLS private key (PEM).
    pub tls_key: Option<PathBuf>,

    /// Path to the API-keys file.
    #[serde(default = "default_api_keys_file")]
    pub api_keys_file: PathBuf,

    /// API-key authentication configuration.
    #[serde(default)]
    pub api_keys: ApiKeysConfig,

    /// Maximum request body size in megabytes.
    #[serde(default = "default_body_limit_mb")]
    pub body_limit_mb: usize,

    /// Request handling timeout in seconds.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Rate limit: requests per second per IP.
    #[serde(default = "default_rate_limit_per_sec")]
    pub rate_limit_per_sec: u32,

    /// Rate limit: maximum burst per IP.
    #[serde(default = "default_rate_limit_burst")]
    pub rate_limit_burst: u32,
}

/// Log output format.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
    Compact,
}

/// API-key authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeysConfig {
    /// Path to the TOML file containing API keys.
    #[serde(default = "default_api_keys_file")]
    pub file: PathBuf,

    /// Optional inline API keys configuration supplied via the
    /// `VICO_VEE_API_KEYS` environment variable.
    #[serde(default)]
    pub env_override: Option<String>,

    /// Require authentication even when no API keys are configured.
    #[serde(default)]
    pub require_auth: bool,
}

impl Default for ApiKeysConfig {
    fn default() -> Self {
        Self {
            file: default_api_keys_file(),
            env_override: None,
            require_auth: false,
        }
    }
}

fn default_port() -> u16 {
    9987
}

fn default_bind() -> String {
    "0.0.0.0".to_string()
}

fn vee_data_dir() -> PathBuf {
    crate::paths::vee_data_dir()
}

fn vee_config_dir() -> PathBuf {
    crate::paths::vee_config_dir()
}

fn default_ollama_url() -> String {
    "http://127.0.0.1:11434".to_string()
}

fn default_api_keys_file() -> PathBuf {
    crate::paths::vee_config_dir().join("api_keys.toml")
}

fn default_body_limit_mb() -> usize {
    16
}

fn default_request_timeout_secs() -> u64 {
    30
}

fn default_rate_limit_per_sec() -> u32 {
    10
}

fn default_rate_limit_burst() -> u32 {
    50
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: default_port(),
            bind: default_bind(),
            data_dir: vee_data_dir(),
            config_dir: vee_config_dir(),
            ollama_url: default_ollama_url(),
            log_format: LogFormat::default(),
            tls_cert: None,
            tls_key: None,
            api_keys_file: default_api_keys_file(),
            api_keys: ApiKeysConfig::default(),
            body_limit_mb: default_body_limit_mb(),
            request_timeout_secs: default_request_timeout_secs(),
            rate_limit_per_sec: default_rate_limit_per_sec(),
            rate_limit_burst: default_rate_limit_burst(),
        }
    }
}

/// CLI overrides parsed with `clap`.
#[derive(Debug, Clone, Default, clap::Parser)]
#[command(name = "vico-vee", about = "ViCo Execution Environment service")]
pub struct Cli {
    /// TCP port to listen on.
    #[arg(long, env = "VICO_VEE_PORT")]
    pub port: Option<u16>,

    /// Interface address to bind to.
    #[arg(long, env = "VICO_VEE_BIND")]
    pub bind: Option<String>,

    /// Base directory for persistent data.
    #[arg(long, env = "VICO_VEE_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Base directory for configuration files.
    #[arg(long, env = "VICO_VEE_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,

    /// URL of the local Ollama instance.
    #[arg(long, env = "VICO_VEE_OLLAMA_URL")]
    pub ollama_url: Option<String>,

    /// Log format: pretty, json, compact.
    #[arg(long, env = "VICO_VEE_LOG_FORMAT")]
    pub log_format: Option<String>,

    /// Path to TLS certificate (PEM).
    #[arg(long, env = "VICO_VEE_TLS_CERT")]
    pub tls_cert: Option<PathBuf>,

    /// Path to TLS private key (PEM).
    #[arg(long, env = "VICO_VEE_TLS_KEY")]
    pub tls_key: Option<PathBuf>,

    /// Path to the API-keys file.
    #[arg(long, env = "VICO_VEE_API_KEYS_FILE")]
    pub api_keys_file: Option<PathBuf>,

    /// Maximum request body size in megabytes.
    #[arg(long, env = "VICO_VEE_BODY_LIMIT_MB")]
    pub body_limit_mb: Option<usize>,

    /// Request handling timeout in seconds.
    #[arg(long, env = "VICO_VEE_REQUEST_TIMEOUT_SECS")]
    pub request_timeout_secs: Option<u64>,

    /// Rate limit: requests per second per IP.
    #[arg(long, env = "VICO_VEE_RATE_LIMIT_PER_SEC")]
    pub rate_limit_per_sec: Option<u32>,

    /// Rate limit: maximum burst per IP.
    #[arg(long, env = "VICO_VEE_RATE_LIMIT_BURST")]
    pub rate_limit_burst: Option<u32>,
}

impl Config {
    /// Load configuration from defaults, config file, environment, and CLI.
    pub fn load(cli: Option<Cli>) -> Result<Self, String> {
        let mut config = Self::default();

        // Layer 1: config file in config dir.
        let config_dir = cli
            .as_ref()
            .and_then(|c| c.config_dir.clone())
            .unwrap_or_else(vee_config_dir);
        if let Ok(file_cfg) = Self::from_file_dir(&config_dir) {
            config = file_cfg;
        }

        // Layer 2: environment variables.
        config.apply_env()?;

        // Layer 3: CLI overrides.
        if let Some(cli) = cli {
            config.apply_cli(cli);
        }

        Ok(config)
    }

    /// Load configuration from environment variables only.
    pub fn from_env() -> Result<Self, String> {
        let mut config = Self::default();
        config.apply_env()?;
        Ok(config)
    }

    /// Attempt to read `config.toml` or `config.yaml` from a directory.
    fn from_file_dir(dir: &std::path::Path) -> Result<Self, String> {
        let toml = dir.join("config.toml");
        let yaml = dir.join("config.yaml");
        let yml = dir.join("config.yml");

        if toml.exists() {
            let text = std::fs::read_to_string(&toml)
                .map_err(|e| format!("read {}: {}", toml.display(), e))?;
            toml::from_str(&text).map_err(|e| format!("parse {}: {}", toml.display(), e))
        } else if yaml.exists() {
            let text = std::fs::read_to_string(&yaml)
                .map_err(|e| format!("read {}: {}", yaml.display(), e))?;
            serde_yaml::from_str(&text).map_err(|e| format!("parse {}: {}", yaml.display(), e))
        } else if yml.exists() {
            let text = std::fs::read_to_string(&yml)
                .map_err(|e| format!("read {}: {}", yml.display(), e))?;
            serde_yaml::from_str(&text).map_err(|e| format!("parse {}: {}", yml.display(), e))
        } else {
            Err(format!("no config file found in {}", dir.display()))
        }
    }

    fn apply_env(&mut self) -> Result<(), String> {
        if let Ok(v) = std::env::var("VICO_VEE_PORT") {
            self.port = v.parse().map_err(|e| format!("VICO_VEE_PORT: {e}"))?;
        }
        if let Ok(v) = std::env::var("VICO_VEE_BIND") {
            self.bind = v;
        }
        if let Ok(v) = std::env::var("VICO_VEE_DATA_DIR") {
            self.data_dir = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("VICO_VEE_CONFIG_DIR") {
            self.config_dir = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("VICO_VEE_OLLAMA_URL") {
            self.ollama_url = v;
        }
        if let Ok(v) = std::env::var("VICO_VEE_LOG_FORMAT") {
            self.log_format = parse_log_format(&v)?;
        }
        if let Ok(v) = std::env::var("VICO_VEE_TLS_CERT") {
            self.tls_cert = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("VICO_VEE_TLS_KEY") {
            self.tls_key = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("VICO_VEE_API_KEYS_FILE") {
            self.api_keys_file = PathBuf::from(v);
        }
        if let Ok(v) = std::env::var("VICO_VEE_BODY_LIMIT_MB") {
            self.body_limit_mb = v
                .parse()
                .map_err(|e| format!("VICO_VEE_BODY_LIMIT_MB: {e}"))?;
        }
        if let Ok(v) = std::env::var("VICO_VEE_REQUEST_TIMEOUT_SECS") {
            self.request_timeout_secs = v
                .parse()
                .map_err(|e| format!("VICO_VEE_REQUEST_TIMEOUT_SECS: {e}"))?;
        }
        if let Ok(v) = std::env::var("VICO_VEE_RATE_LIMIT_PER_SEC") {
            self.rate_limit_per_sec = v
                .parse()
                .map_err(|e| format!("VICO_VEE_RATE_LIMIT_PER_SEC: {e}"))?;
        }
        if let Ok(v) = std::env::var("VICO_VEE_RATE_LIMIT_BURST") {
            self.rate_limit_burst = v
                .parse()
                .map_err(|e| format!("VICO_VEE_RATE_LIMIT_BURST: {e}"))?;
        }
        Ok(())
    }

    fn apply_cli(&mut self, cli: Cli) {
        if let Some(v) = cli.port {
            self.port = v;
        }
        if let Some(v) = cli.bind {
            self.bind = v;
        }
        if let Some(v) = cli.data_dir {
            self.data_dir = v;
        }
        if let Some(v) = cli.config_dir {
            self.config_dir = v;
        }
        if let Some(v) = cli.ollama_url {
            self.ollama_url = v;
        }
        if let Some(v) = cli.log_format {
            self.log_format = parse_log_format(&v).unwrap_or_else(|e| {
                tracing::warn!("invalid --log-format {v}: {e}; using default");
                self.log_format.clone()
            });
        }
        if cli.tls_cert.is_some() {
            self.tls_cert = cli.tls_cert;
        }
        if cli.tls_key.is_some() {
            self.tls_key = cli.tls_key;
        }
        if let Some(v) = cli.api_keys_file {
            self.api_keys_file = v;
        }
        if let Some(v) = cli.body_limit_mb {
            self.body_limit_mb = v;
        }
        if let Some(v) = cli.request_timeout_secs {
            self.request_timeout_secs = v;
        }
        if let Some(v) = cli.rate_limit_per_sec {
            self.rate_limit_per_sec = v;
        }
        if let Some(v) = cli.rate_limit_burst {
            self.rate_limit_burst = v;
        }
    }
}

fn parse_log_format(s: &str) -> Result<LogFormat, String> {
    match s.to_lowercase().as_str() {
        "pretty" => Ok(LogFormat::Pretty),
        "json" => Ok(LogFormat::Json),
        "compact" => Ok(LogFormat::Compact),
        other => Err(format!("unknown log format: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn config_defaults_are_sensible() {
        let cfg = Config::default();
        assert_eq!(cfg.port, 9987);
        assert_eq!(cfg.bind, "0.0.0.0");
        assert_eq!(cfg.body_limit_mb, 16);
        assert_eq!(cfg.request_timeout_secs, 30);
        assert_eq!(cfg.rate_limit_per_sec, 10);
        assert_eq!(cfg.rate_limit_burst, 50);
    }

    #[test]
    fn config_cli_overrides_defaults() {
        let cli = Cli {
            port: Some(1234),
            bind: Some("127.0.0.1".to_string()),
            log_format: Some("json".to_string()),
            ..Default::default()
        };
        let cfg = Config::load(Some(cli)).unwrap();
        assert_eq!(cfg.port, 1234);
        assert_eq!(cfg.bind, "127.0.0.1");
        assert_eq!(cfg.log_format, LogFormat::Json);
    }

    #[test]
    fn config_file_is_respected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(tmp.path().join("config.toml")).unwrap();
        writeln!(f, "port = 7777").unwrap();
        writeln!(f, "bind = \"127.0.0.1\"").unwrap();
        writeln!(f, "body_limit_mb = 8").unwrap();
        drop(f);

        let cli = Cli {
            config_dir: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };

        let cfg = Config::load(Some(cli)).unwrap();
        assert_eq!(cfg.port, 7777);
        assert_eq!(cfg.bind, "127.0.0.1");
        assert_eq!(cfg.body_limit_mb, 8);
        // Defaults preserved for unspecified fields.
        assert_eq!(cfg.request_timeout_secs, 30);
    }

    #[test]
    fn config_cli_beats_env_and_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(tmp.path().join("config.toml")).unwrap();
        writeln!(f, "port = 7777").unwrap();
        drop(f);

        let cli = Cli {
            config_dir: Some(tmp.path().to_path_buf()),
            port: Some(9000),
            ..Default::default()
        };

        let cfg = Config::load(Some(cli)).unwrap();
        assert_eq!(cfg.port, 9000);
    }
}
