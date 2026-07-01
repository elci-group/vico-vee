use crate::sandbox::landlock::apply_landlock;
use crate::sandbox::seccomp::apply_seccomp_filter;
use crate::sandbox::types::{SandboxConfig, SandboxResult};
use crate::types::ExecutionBudget;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

/// Apply sandbox restrictions to a command via `pre_exec`.
///
/// This is the same layering used by [`run_sandboxed`], but exposed so callers
/// can spawn and manage the child process themselves (e.g., to implement
/// cancellation/timeouts with a process kill).
pub fn apply_sandbox_pre_exec(cmd: &mut Command, config: &SandboxConfig) {
    let budget = config.budget.clone();
    let block_network = config.block_network;
    let work_dir = config.work_dir.clone();
    let output_dir = config.output_dir.clone();
    let input_paths = config.input_paths.clone();
    let executable_paths = config.executable_paths.clone();

    unsafe {
        cmd.pre_exec(move || {
            // Fail closed only when VICO_SANDBOX_STRICT is set. In normal local
            // desktop use, sandbox layers are best-effort: if Landlock/seccomp
            // are unavailable or a limit is rejected by the kernel, we log a
            // warning and continue rather than making every shell command fail.
            let strict_mode = std::env::var("VICO_SANDBOX_STRICT").is_ok();

            // Layer 1: rlimits (always applied)
            if let Err(e) = apply_rlimits(&budget) {
                if strict_mode {
                    return Err(std::io::Error::other(format!(
                        "VEE sandbox rlimit failed: {}",
                        e
                    )));
                }
                tracing::warn!("VEE sandbox rlimit failed (continuing without it): {}", e);
            }

            // Layer 2: Landlock filesystem confinement
            if let Err(e) = apply_landlock(&work_dir, &output_dir, &input_paths, &executable_paths)
            {
                if strict_mode {
                    return Err(std::io::Error::other(format!(
                        "VEE sandbox Landlock failed: {}",
                        e
                    )));
                }
                tracing::warn!("VEE sandbox Landlock failed (continuing without it): {}", e);
            }

            // Layer 3: seccomp-bpf syscall filtering
            // Seccomp is the most fragile layer: many ordinary CLI tools
            // (neofetch, imagemagick, compilers, etc.) need syscalls that are
            // tedious to enumerate and keep current. Only apply it in strict
            // mode; otherwise rely on rlimits + Landlock for containment.
            if strict_mode {
                if let Err(e) = apply_seccomp_filter(block_network) {
                    return Err(std::io::Error::other(format!(
                        "VEE sandbox seccomp failed: {}",
                        e
                    )));
                }
            }

            Ok(())
        });
    }
}

/// Run a command inside the sandbox.
pub fn run_sandboxed(mut cmd: Command, config: &SandboxConfig) -> Result<SandboxResult, String> {
    let start = std::time::Instant::now();
    let mut layers = Vec::new();
    let errors = Vec::new();

    // Pre-exec sandbox setup (runs in child process)
    apply_sandbox_pre_exec(&mut cmd, config);

    // Run the command
    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|child| child.wait_with_output())
        .map_err(|e| format!("Failed to spawn sandboxed process: {}", e))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code();

    // Estimate memory peak (best effort via /proc)
    let memory_peak_kb = estimate_memory_peak();

    // Determine which layers actually applied by checking stderr for our markers
    // In a real implementation we'd use a pipe or shared memory; here we scan stderr
    if stderr.contains("[VEE-SANDBOX]") {
        // Errors were logged; we still consider rlimit as applied
    }
    layers.push("rlimit".into());

    Ok(SandboxResult {
        stdout,
        stderr,
        exit_code,
        duration_ms,
        memory_peak_kb,
        sandbox_layers_applied: layers,
        sandbox_errors: errors,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 1: rlimit
// ─────────────────────────────────────────────────────────────────────────────

fn apply_rlimits(budget: &ExecutionBudget) -> Result<(), String> {
    use libc::{setrlimit, RLIMIT_AS, RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_NOFILE};

    // CPU time (seconds)
    let cpu_limit = libc::rlimit {
        rlim_cur: budget.cpu_seconds,
        rlim_max: budget.cpu_seconds + 5, // hard limit slightly higher
    };
    if unsafe { setrlimit(RLIMIT_CPU, &cpu_limit) } != 0 {
        return Err("RLIMIT_CPU failed".into());
    }

    // Memory (bytes) — RLIMIT_AS = address space limit
    let mem_bytes = budget.memory_mb * 1024 * 1024;
    let mem_limit = libc::rlimit {
        rlim_cur: mem_bytes,
        rlim_max: mem_bytes + 50 * 1024 * 1024,
    };
    if unsafe { setrlimit(RLIMIT_AS, &mem_limit) } != 0 {
        return Err("RLIMIT_AS failed".into());
    }

    // File size (bytes)
    let disk_bytes = budget.disk_mb * 1024 * 1024;
    let fs_limit = libc::rlimit {
        rlim_cur: disk_bytes,
        rlim_max: disk_bytes,
    };
    if unsafe { setrlimit(RLIMIT_FSIZE, &fs_limit) } != 0 {
        return Err("RLIMIT_FSIZE failed".into());
    }

    // Do not restrict NPROC: shell commands legitimately fork (e.g.,
    // `/bin/sh -c "find /tmp | wc -l"`), and a low limit breaks them.
    // Fork-bomb protection is handled by the wall-clock timeout instead.
    // let nproc_limit = libc::rlimit {
    //     rlim_cur: 1024,
    //     rlim_max: 1024,
    // };
    // unsafe { setrlimit(RLIMIT_NPROC, &nproc_limit) }; // best effort

    // Limit open files. Use a generous ceiling: the parent server may have
    // many open sockets/files that are inherited by the child, and Landlock
    // itself needs a file descriptor to create the ruleset.
    let nofile_limit = libc::rlimit {
        rlim_cur: 4096,
        rlim_max: 4096,
    };
    unsafe { setrlimit(RLIMIT_NOFILE, &nofile_limit) }; // best effort

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn estimate_memory_peak() -> u64 {
    // Best effort: read VmPeak from /proc/self/status
    if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if line.starts_with("VmPeak:") {
                let parts: Vec<_> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        return kb;
                    }
                }
            }
        }
    }
    0
}
