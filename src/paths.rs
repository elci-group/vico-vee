//! Portable path helpers for vico-vee.
//!
//! All functions return sensible fallbacks so the service still starts on
//! machines where the standard XDG directories cannot be determined.

use std::path::PathBuf;

/// Base directory for vico-vee persistent data.
///
/// Falls back to the system temp dir only as a last resort. Callers should
/// create the directory before use.
///
/// Can be overridden with the `VICO_VEE_DATA_DIR` environment variable.
pub fn vee_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VICO_VEE_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("vico-vee")
}

/// Base directory for vico-vee configuration files.
///
/// Can be overridden with the `VICO_VEE_CONFIG_DIR` environment variable.
pub fn vee_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VICO_VEE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("vico-vee")
}
