use std::path::{Path, PathBuf};

#[repr(C)]
struct LandlockRulesetAttr {
    handled_access_fs: u64,
}

const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;

#[repr(C)]
struct LandlockPathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

// Landlock access rights (from linux/landlock.h)
const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
const _LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
const _LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
const _LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
const _LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
const _LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;

pub(crate) fn apply_landlock(
    work_dir: &Path,
    output_dir: &Path,
    input_paths: &[PathBuf],
    executable_paths: &[PathBuf],
) -> Result<(), String> {
    // Landlock syscalls on x86_64
    const SYS_LANDLOCK_CREATE_RULESET: i64 = 444;
    const SYS_LANDLOCK_ADD_RULE: i64 = 445;
    const SYS_LANDLOCK_RESTRICT_SELF: i64 = 446;

    let handled_access = LANDLOCK_ACCESS_FS_EXECUTE
        | LANDLOCK_ACCESS_FS_WRITE_FILE
        | LANDLOCK_ACCESS_FS_READ_FILE
        | LANDLOCK_ACCESS_FS_READ_DIR
        | LANDLOCK_ACCESS_FS_REMOVE_DIR
        | LANDLOCK_ACCESS_FS_REMOVE_FILE
        | LANDLOCK_ACCESS_FS_MAKE_DIR
        | LANDLOCK_ACCESS_FS_MAKE_REG;

    let attr = LandlockRulesetAttr {
        handled_access_fs: handled_access,
    };

    let ruleset_fd = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr,
            std::mem::size_of::<LandlockRulesetAttr>(),
            0u32,
        )
    };

    if ruleset_fd < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("landlock_create_ruleset failed: {}", err));
    }
    let ruleset_fd = ruleset_fd as i32;

    // Helper to add a path rule
    let add_path = |path: &Path, access: u64| -> Result<(), String> {
        let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|e| format!("Invalid path: {}", e))?;
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_PATH | libc::O_CLOEXEC | libc::O_DIRECTORY,
            )
        };
        if fd < 0 {
            // Try without O_DIRECTORY
            let fd2 = unsafe { libc::open(c_path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
            if fd2 < 0 {
                let err = std::io::Error::last_os_error();
                return Err(format!("open({}) failed: {}", path.display(), err));
            }
            let beneath = LandlockPathBeneathAttr {
                allowed_access: access,
                parent_fd: fd2,
            };
            let ret = unsafe {
                libc::syscall(
                    SYS_LANDLOCK_ADD_RULE,
                    ruleset_fd,
                    LANDLOCK_RULE_PATH_BENEATH,
                    &beneath,
                    0u32,
                )
            };
            unsafe { libc::close(fd2) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                return Err(format!("landlock_add_rule failed: {}", err));
            }
            return Ok(());
        }
        let beneath = LandlockPathBeneathAttr {
            allowed_access: access,
            parent_fd: fd,
        };
        let ret = unsafe {
            libc::syscall(
                SYS_LANDLOCK_ADD_RULE,
                ruleset_fd,
                LANDLOCK_RULE_PATH_BENEATH,
                &beneath,
                0u32,
            )
        };
        unsafe { libc::close(fd) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            return Err(format!("landlock_add_rule failed: {}", err));
        }
        Ok(())
    };

    // Allow read-write on work_dir
    let rw_access = LANDLOCK_ACCESS_FS_READ_FILE
        | LANDLOCK_ACCESS_FS_WRITE_FILE
        | LANDLOCK_ACCESS_FS_READ_DIR
        | LANDLOCK_ACCESS_FS_MAKE_DIR
        | LANDLOCK_ACCESS_FS_MAKE_REG
        | LANDLOCK_ACCESS_FS_REMOVE_FILE
        | LANDLOCK_ACCESS_FS_REMOVE_DIR;
    add_path(work_dir, rw_access)?;

    // Allow write-only on output_dir
    let out_access = LANDLOCK_ACCESS_FS_READ_DIR
        | LANDLOCK_ACCESS_FS_WRITE_FILE
        | LANDLOCK_ACCESS_FS_MAKE_DIR
        | LANDLOCK_ACCESS_FS_MAKE_REG;
    add_path(output_dir, out_access)?;

    // Allow read-only on input paths
    let ro_access = LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR;
    for path in input_paths {
        if let Err(e) = add_path(path, ro_access) {
            tracing::warn!(path = %path.display(), "failed to add Landlock path rule: {}", e);
        }
    }

    // Allow read + execute on executable paths (system binary directories)
    let rx_access =
        LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR | LANDLOCK_ACCESS_FS_EXECUTE;
    for path in executable_paths {
        if let Err(e) = add_path(path, rx_access) {
            tracing::warn!(path = %path.display(), "failed to add Landlock executable path rule: {}", e);
        }
    }

    // Apply restrictions to current thread
    let ret = unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset_fd, 0u32) };
    unsafe { libc::close(ruleset_fd) };

    if ret < 0 {
        let err = std::io::Error::last_os_error();
        return Err(format!("landlock_restrict_self failed: {}", err));
    }

    Ok(())
}
