//! Layered configuration for the standalone `vico-vee` service.
//!
//! Priority (lowest to highest):
//!   1. Built-in defaults
//!   2. Configuration file (`config.toml` or `config.yaml` in the config dir)
//!   3. Environment variables prefixed with `VICO_VEE_`
//!   4. Command-line arguments

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Default bind address.
pub const DEFAULT_BIND: &str = "0.0.0.0";
/// Default HTTP port.
pub const DEFAULT_PORT: u16 = 9987;
/// Default Ollama base URL.
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

/// Server configuration, loaded from defaults → file → env → CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VeeConfig {
    /// Address to bind the HTTP(S) listener to.
    #[serde(default = "default_bind")]
    pub bind: String,

    /// Port to listen on.
    #[serde(default = "default_port")]
    pub port: u16,

    /// Directory for persistent data (databases, blobs, keys).
    #[serde(default = "vee_data_dir")]
    pub data_dir: PathBuf,

    /// Directory for configuration files.
    #[serde(default = "vee_config_dir")]
    pub config_dir: PathBuf,

    /// Base URL for the local Ollama instance used by ODIN.
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,

    /// Optional path to a PEM-encoded TLS certificate. If both cert and key are
    /// provided the service serves HTTPS; otherwise it serves plain HTTP.
    #[serde(default)]
    pub tls_cert: Option<PathBuf>,

    /// Optional path to a PEM-encoded TLS private key.
    #[serde(default)]
    pub tls_key: Option<PathBuf>,

    /// Path to the API keys file. When absent, no API-key authentication is
    /// enforced (useful for local development only).
    #[serde(default)]
    pub api_keys_file: Option<PathBuf>,

    /// Log output format.
    #[serde(default)]
    pub log_format: LogFormat,

    /// Maximum request body size in megabytes.
    #[serde(default = "default_body_limit_mb")]
    pub request_body_limit_mb: usize,

    /// Request handling timeout in seconds.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Per-IP rate limit: sustained requests per second.
    #[serde(default = "default_rate_limit_rps")]
    pub rate_limit_rps: u32,

    /// Per-IP rate limit: maximum burst size.
    #[serde(default = "default_rate_limit_burst")]
    pub rate_limit_burst: u32,

    /// Maximum number of seconds to wait for in-flight executions during graceful
    /// shutdown.
    #[serde(default = "default_graceful_shutdown_secs")]
    pub graceful_shutdown_secs: u64,
}

/// Log output format.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable plain text logs.
    #[default]
    Text,
    /// Structured JSON logs.
    Json,
}

impl std::fmt::Display for LogFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogFormat::Text => write!(f, "text"),
            LogFormat::Json => write!(f, "json"),
        }
    }
}

impl std::str::FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "text" => Ok(LogFormat::Text),
            "json" => Ok(LogFormat::Json),
            _ => Err(format!("unknown log format: {s}")),
        }
    }
}

fn default_bind() -> String {
    DEFAULT_BIND.into()
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_ollama_url() -> String {
    DEFAULT_OLLAMA_URL.into()
}

fn default_body_limit_mb() -> usize {
    16
}

fn default_request_timeout_secs() -> u64 {
    30
}

fn default_rate_limit_rps() -> u32 {
    10
}

fn default_rate_limit_burst() -> u32 {
    20
}

fn default_graceful_shutdown_secs() -> u64 {
    30
}

/// Command-line arguments parsed with `clap`.
#[derive(Debug, Parser)]
#[command(name = "vico-vee")]
#[command(about = "ViCo Execution Environment — standalone sandboxed execution service")]
pub struct Cli {
    /// Address to bind the HTTP(S) listener to.
    #[arg(long, env = "VICO_VEE_BIND")]
    pub bind: Option<String>,

    /// Port to listen on.
    #[arg(short, long, env = "VICO_VEE_PORT")]
    pub port: Option<u16>,

    /// Directory for persistent data.
    #[arg(long, env = "VICO_VEE_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Directory for configuration files.
    #[arg(long, env = "VICO_VEE_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,

    /// Base URL for the local Ollama instance.
    #[arg(long, env = "VICO_VEE_OLLAMA_URL")]
    pub ollama_url: Option<String>,

    /// Path to a PEM-encoded TLS certificate.
    #[arg(long, env = "VICO_VEE_TLS_CERT")]
    pub tls_cert: Option<PathBuf>,

    /// Path to a PEM-encoded TLS private key.
    #[arg(long, env = "VICO_VEE_TLS_KEY")]
    pub tls_key: Option<PathBuf>,

    /// Path to the API keys file.
    #[arg(long, env = "VICO_VEE_API_KEYS_FILE")]
    pub api_keys_file: Option<PathBuf>,

    /// Log output format (text or json).
    #[arg(long, env = "VICO_VEE_LOG_FORMAT")]
    pub log_format: Option<LogFormat>,
}

impl Default for VeeConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            port: default_port(),
            data_dir: crate::paths::vee_data_dir(),
            config_dir: crate::paths::vee_config_dir(),
            ollama_url: default_ollama_url(),
            tls_cert: None,
            tls_key: None,
            api_keys_file: None,
            log_format: LogFormat::default(),
            request_body_limit_mb: default_body_limit_mb(),
            request_timeout_secs: default_request_timeout_secs(),
            rate_limit_rps: default_rate_limit_rps(),
            rate_limit_burst: default_rate_limit_burst(),
            graceful_shutdown_secs: default_graceful_shutdown_secs(),
        }
    }
}

impl VeeConfig {
    /// Load configuration with the standard precedence.
    pub fn load() -> Result<Self, String> {
        let cli = Cli::parse();
        let mut config = Self::default();

        // Layer 1: configuration file in the config directory.
        let config_dir = cli.config_dir.clone().unwrap_or_else(vee_config_dir);
        config = load_config_file(config, &config_dir)?;

        // Layer 2: environment variables (already reflected in `cli` via clap's `env`).
        // We still apply the raw env values so the file layer can be overridden.
        config = apply_cli(config, &cli, false);

        Ok(config)
    }

    /// Load configuration for tests, using the provided CLI overrides only.
    pub fn load_for_test(cli: Cli) -> Result<Self, String> {
        let mut config = Self::default();
        let config_dir = cli.config_dir.clone().unwrap_or_else(vee_config_dir);
        config = load_config_file(config, &config_dir)?;
        config = apply_cli(config, &cli, true);
        Ok(config)
    }
}

fn load_config_file(mut config: VeeConfig, config_dir: &std::path::Path) -> Result<VeeConfig, String> {
    if !config_dir.exists() {
        return Ok(config);
    }

    let candidates = ["config.toml", "config.yaml"];
    for name in candidates {
        let path = config_dir.join(name);
        if !path.exists() {
            continue;
        }

        let contents = std::fs::read_to_string(&path)
            .map_err(|e| format!("read config file {}: {}", path.display(), e))?;

        let file_config: VeeConfig = if name.ends_with(".toml") {
            toml::from_str(&contents)
                .map_err(|e| format!("parse config file {}: {}", path.display(), e))?
        } else {
            serde_yaml::from_str(&contents)
                .map_err(|e| format!("parse config file {}: {}", path.display(), e))?
        };

        config = merge(config, file_config);
        break;
    }

    Ok(config)
}

fn merge(base: VeeConfig, overlay: VeeConfig) -> VeeConfig {
    VeeConfig {
        bind: if_overlay(overlay.bind, base.bind, overlay.bind != default_bind()),
        port: if_overlay(overlay.port, base.port, overlay.port != default_port()),
        data_dir: if_overlay(overlay.data_dir, base.data_dir, true),
        config_dir: if_overlay(overlay.config_dir, base.config_dir, true),
        ollama_url: if_overlay(
            overlay.ollama_url,
            base.ollama_url,
            overlay.ollama_url != default_ollama_url(),
        ),
        tls_cert: overlay.tls_cert.or(base.tls_cert),
        tls_key: overlay.tls_key.or(base.tls_key),
        api_keys_file: overlay.api_keys_file.or(base.api_keys_file),
        log_format: if_overlay(
            overlay.log_format,
            base.log_format,
            overlay.log_format != LogFormat::default(),
        ),
        request_body_limit_mb: if_overlay(
            overlay.request_body_limit_mb,
            base.request_body_limit_mb,
            overlay.request_body_limit_mb != default_body_limit_mb(),
        ),
        request_timeout_secs: if_overlay(
            overlay.request_timeout_secs,
            base.request_timeout_secs,
            overlay.request_timeout_secs != default_request_timeout_secs(),
        ),
        rate_limit_rps: if_overlay(
            overlay.rate_limit_rps,
            base.rate_limit_rps,
            overlay.rate_limit_rps != default_rate_limit_rps(),
        ),
        rate_limit_burst: if_overlay(
            overlay.rate_limit_burst,
            base.rate_limit_burst,
            overlay.rate_limit_burst != default_rate_limit_burst(),
        ),
        graceful_shutdown_secs: if_overlay(
            overlay.graceful_shutdown_secs,
            base.graceful_shutdown_secs,
            overlay.graceful_shutdown_secs != default_graceful_shutdown_secs(),
        ),
    }
}

fn if_overlay<T>(value: T, base: T, changed: bool) -> T {
    if changed {
        value
    } else {
        base
    }
}

fn apply_cli(mut config: VeeConfig, cli: &Cli, apply_all: bool) -> VeeConfig {
    if let Some(bind) = &cli.bind {
        config.bind = bind.clone();
    }
    if let Some(port) = cli.port {
        config.port = port;
    }
    if let Some(data_dir) = &cli.data_dir {
        config.data_dir = data_dir.clone();
    }
    if apply_all {
        if let Some(config_dir) = &cli.config_dir {
            config.config_dir = config_dir.clone();
        }
    }
    if let Some(ollama_url) = &cli.ollama_url {
        config.ollama_url = ollama_url.clone();
    }
    if let Some(tls_cert) = &cli.tls_cert {
        config.tls_cert = Some(tls_cert.clone());
    }
    if let Some(tls_key) = &cli.tls_key {
        config.tls_key = Some(tls_key.clone());
    }
    if let Some(api_keys_file) = &cli.api_keys_file {
        config.api_keys_file = Some(api_keys_file.clone());
    }
    if let Some(log_format) = &cli.log_format {
        config.log_format = log_format.clone();
    }
    config
}

pub fn vee_data_dir() -> PathBuf {
    crate::paths::vee_data_dir()
}

pub fn vee_config_dir() -> PathBuf {
    crate::paths::vee_config_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_sensible() {
        let config = VeeConfig::default();
        assert_eq!(config.bind, "0.0.0.0");
        assert_eq!(config.port, 9987);
        assert_eq!(config.ollama_url, "http://127.0.0.1:11434");
        assert_eq!(config.request_body_limit_mb, 16);
        assert_eq!(config.log_format, LogFormat::Text);
    }

    #[test]
    fn config_cli_overrides_defaults() {
        let cli = Cli {
            bind: Some("127.0.0.1".into()),
            port: Some(1234),
            data_dir: Some(PathBuf::from("/tmp/vee-data")),
            config_dir: Some(PathBuf::from("/tmp/vee-config")),
            ollama_url: Some("http://ollama:11434".into()),
            tls_cert: Some(PathBuf::from("/tmp/cert.pem")),
            tls_key: Some(PathBuf::from("/tmp/key.pem")),
            api_keys_file: Some(PathBuf::from("/tmp/keys.toml")),
            log_format: Some(LogFormat::Json),
        };
        let config = VeeConfig::load_for_test(cli).unwrap();
        assert_eq!(config.bind, "127.0.0.1");
        assert_eq!(config.port, 1234);
        assert_eq!(config.data_dir, PathBuf::from("/tmp/vee-data"));
        assert_eq!(config.ollama_url, "http://ollama:11434");
        assert_eq!(config.log_format, LogFormat::Json);
        assert_eq!(config.tls_cert, Some(PathBuf::from("/tmp/cert.pem")));
    }

    #[test]
    fn config_file_overrides_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            r#"
port = 7777
bind = "127.0.0.1"
log_format = "json"
"#,
        )
        .unwrap();

        let cli = Cli {
            bind: None,
            port: None,
            data_dir: None,
            config_dir: Some(tmp.path().into()),
            ollama_url: None,
            tls_cert: None,
            tls_key: None,
            api_keys_file: None,
            log_format: None,
        };
        let config = VeeConfig::load_for_test(cli).unwrap();
        assert_eq!(config.port, 7777);
        assert_eq!(config.bind, "127.0.0.1");
        assert_eq!(config.log_format, LogFormat::Json);
    }

    #[test]
    fn config_cli_beats_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            r#"
port = 7777
"#,
        )
        .unwrap();

        let cli = Cli {
            bind: None,
            port: Some(8888),
            data_dir: None,
            config_dir: Some(tmp.path().into()),
            ollama_url: None,
            tls_cert: None,
            tls_key: None,
            api_keys_file: None,
            log_format: None,
        };
        let config = VeeConfig::load_for_test(cli).unwrap();
        assert_eq!(config.port, 8888);
    }

    #[test]
    fn config_yaml_file_is_supported() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"
port: 6666
ollama_url: "http://yaml:11434"
"#,
        )
        .unwrap();

        let cli = Cli {
            bind: None,
            port: None,
            data_dir: None,
            config_dir: Some(tmp.path().into()),
            ollama_url: None,
            tls_cert: None,
            tls_key: None,
            api_keys_file: None,
            log_format: None,
        };
        let config = VeeConfig::load_for_test(cli).unwrap();
        assert_eq!(config.port, 6666);
        assert_eq!(config.ollama_url, "http://yaml:11434");
    }

    #[test]
    fn config_toml_beats_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("config.toml"), "port = 1111\n").unwrap();
        std::fs::write(tmp.path().join("config.yaml"), "port: 2222\n").unwrap();

        let cli = Cli {
            bind: None,
            port: None,
            data_dir: None,
            config_dir: Some(tmp.path().into()),
            ollama_url: None,
            tls_cert: None,
            tls_key: None,
            api_keys_file: None,
            log_format: None,
        };
        let config = VeeConfig::load_for_test(cli).unwrap();
        assert_eq!(config.port, 1111);
    }
}
