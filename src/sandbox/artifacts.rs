use crate::sandbox::types::{SandboxConfig, SandboxResult};
use crate::types::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build a sandboxed Python command.
pub fn build_python_command(
    source_code: &str,
    work_dir: &Path,
    output_dir: &Path,
) -> Result<(Command, SandboxConfig), String> {
    std::fs::create_dir_all(work_dir).map_err(|e| format!("create work_dir: {}", e))?;
    std::fs::create_dir_all(output_dir).map_err(|e| format!("create output_dir: {}", e))?;

    // Provision the snakepit binary into the sandbox so the script can manage
    // Python packages internally without touching the host system.
    let snakepit_bin_dir = work_dir.join(".snakepit").join("bin");
    std::fs::create_dir_all(&snakepit_bin_dir)
        .map_err(|e| format!("create snakepit bin dir: {}", e))?;
    let snakepit_src = std::env::var_os("SNAKEPIT_BINARY")
        .map(PathBuf::from)
        .or_else(|| which::which("snakepit").ok())
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("bin").join("snakepit")))
        .filter(|p| p.exists())
        .ok_or_else(|| {
            "snakepit binary not found; install it or set SNAKEPIT_BINARY".to_string()
        })?;
    let snakepit_dst = snakepit_bin_dir.join("snakepit");
    std::fs::copy(&snakepit_src, &snakepit_dst).map_err(|e| {
        format!(
            "copy snakepit binary from {} to {}: {}",
            snakepit_src.display(),
            snakepit_dst.display(),
            e
        )
    })?;

    // Write source to work_dir
    let source_path = work_dir.join("script.py");
    std::fs::write(&source_path, source_code).map_err(|e| format!("write source: {}", e))?;

    let path = format!("{}:/usr/bin:/bin", snakepit_bin_dir.display());
    let mut cmd = Command::new("python3");
    cmd.arg(&source_path)
        .current_dir(work_dir)
        .env_clear()
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .env("HOME", work_dir)
        .env("PATH", path);

    let config = SandboxConfig {
        work_dir: work_dir.to_path_buf(),
        output_dir: output_dir.to_path_buf(),
        input_paths: vec![],
        executable_paths: vec![snakepit_dst],
        budget: ExecutionBudget::default(),
        capabilities: vec![],
        block_network: true,
    };

    Ok((cmd, config))
}

/// Parse width/height from PNG or JPEG bytes without adding an image crate.
fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        // IHDR: 4-byte length, 4-byte "IHDR", then width, height (big-endian).
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return Some((width, height));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        // Parse JPEG markers looking for SOF0/SOF2 (0xFFC0 / 0xFFC2).
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if marker == 0xC0 || marker == 0xC2 {
                let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Some((width, height));
            }
            // Skip marker segment: 2-byte length includes the length bytes themselves.
            if marker != 0x00
                && marker != 0x01
                && !(0xD0..=0xD9).contains(&marker)
                && i + 3 < bytes.len()
            {
                let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
                i += 2 + len.max(2);
                continue;
            }
            i += 2;
        }
    }
    None
}

/// Extract artifacts from sandbox result and output directory.
pub fn extract_artifacts(
    result: &SandboxResult,
    output_dir: &Path,
) -> Vec<crate::types::Artifact> {
    let mut artifacts = Vec::new();

    // Always capture stdout as text
    if !result.stdout.is_empty() {
        let line_count = result.stdout.lines().count();
        artifacts.push(Artifact::Text {
            content: result.stdout.clone(),
            format: if result.stdout.trim_start().starts_with("{") {
                TextFormat::Json
            } else {
                TextFormat::Plain
            },
            line_count,
        });
    }

    // Capture stderr as log
    if !result.stderr.is_empty() {
        let entries: Vec<_> = result
            .stderr
            .lines()
            .map(|line| LogEntry {
                timestamp: chrono::Utc::now(),
                level: if line.to_lowercase().contains("error") {
                    LogLevel::Error
                } else if line.to_lowercase().contains("warn") {
                    LogLevel::Warn
                } else {
                    LogLevel::Info
                },
                message: line.to_string(),
                source: "stderr".to_string(),
            })
            .collect();

        let mut level_counts = std::collections::HashMap::new();
        for entry in &entries {
            *level_counts.entry(entry.level.clone()).or_insert(0) += 1;
        }

        artifacts.push(Artifact::Log {
            entries,
            level_counts,
        });
    }

    // Scan output directory for files
    if let Ok(entries) = std::fs::read_dir(output_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    let size = meta.len();
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let mime = match ext {
                        "csv" => "text/csv",
                        "json" => "application/json",
                        "png" => "image/png",
                        "jpg" | "jpeg" => "image/jpeg",
                        "svg" => "image/svg+xml",
                        "txt" => "text/plain",
                        "md" => "text/markdown",
                        "yaml" | "yml" => "application/yaml",
                        _ => "application/octet-stream",
                    };

                    // Try to read content for small files
                    if size < 5 * 1024 * 1024 {
                        if let Ok(bytes) = std::fs::read(&path) {
                            if ext == "png" || ext == "jpg" || ext == "jpeg" {
                                let (width, height) = image_dimensions(&bytes).unwrap_or((0, 0));
                                artifacts.push(Artifact::Image {
                                    format: match ext {
                                        "png" => ImageFormat::Png,
                                        "jpg" | "jpeg" => ImageFormat::Jpg,
                                        _ => ImageFormat::Png,
                                    },
                                    width,
                                    height,
                                    bytes,
                                });
                                continue;
                            }

                            if ext == "json" {
                                if let Ok(text) = String::from_utf8(bytes.clone()) {
                                    if let Ok(value) =
                                        serde_json::from_str::<serde_json::Value>(&text)
                                    {
                                        let hash = format!("{:x}", Sha256::digest(&bytes));
                                        artifacts.push(Artifact::Json {
                                            value,
                                            schema_hash: hash[..16].to_string(),
                                        });
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // File reference for large or binary files
                    let hash = if let Ok(bytes) = std::fs::read(&path) {
                        format!("{:x}", Sha256::digest(&bytes))
                    } else {
                        "unknown".to_string()
                    };

                    artifacts.push(Artifact::File {
                        path: path.clone(),
                        size_bytes: size,
                        mime_type: mime.to_string(),
                        hash,
                    });
                }
            }
        }
    }

    artifacts
}
