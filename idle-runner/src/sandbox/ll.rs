use std::path::Path;

pub const ACCESS_FS_EXECUTE: u64 = 1 << 0;
pub const ACCESS_FS_READ_FILE: u64 = 1 << 2;
pub const ACCESS_FS_READ_DIR: u64 = 1 << 3;
/// Every v1 right: execute, read, write, all make/remove rights.
pub const ACCESS_FS_ALL_V1: u64 = 0x1FFF;
/// `AccessFs::from_read(ABI::V1)` — read + traverse + execute (dlopen needs this).
pub const READ_EXEC: u64 = ACCESS_FS_EXECUTE | ACCESS_FS_READ_FILE | ACCESS_FS_READ_DIR;
/// `AccessFs::from_read | AccessFs::from_write` — all v1 rights.
pub const READ_WRITE: u64 = ACCESS_FS_ALL_V1;
/// `LANDLOCK_RULE_PATH_BENEATH`.
pub const RULE_PATH_BENEATH: libc::c_int = 1;
#[repr(C)]
pub struct RulesetAttr {
    handled_access_fs: u64,
}

#[repr(C)]
pub struct PathBeneathAttr {
    allowed_access: u64,
    parent_fd: libc::c_int,
}

/// Close an fd on drop — keeps `?` early-returns from leaking descriptors.
pub struct FdGuard(pub std::os::fd::RawFd);
impl Drop for FdGuard {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

/// `landlock_create_ruleset` syscall → ruleset fd.
pub fn ll_create_ruleset() -> Result<std::os::fd::RawFd, String> {
    let attr = RulesetAttr {
        handled_access_fs: ACCESS_FS_ALL_V1,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &attr,
            std::mem::size_of::<RulesetAttr>(),
            0,
        )
    };
    if fd < 0 {
        Err(format!(
            "Failed to create ruleset: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(fd as std::os::fd::RawFd)
    }
}

/// `landlock_add_rule(fd, PATH_BENEATH, {access, parent_fd})`.
pub fn ll_add_rule(
    ruleset_fd: std::os::fd::RawFd,
    access: u64,
    parent_fd: std::os::fd::RawFd,
) -> Result<(), String> {
    let attr = PathBeneathAttr {
        allowed_access: access,
        parent_fd,
    };
    let r = unsafe {
        libc::syscall(
            libc::SYS_landlock_add_rule,
            ruleset_fd,
            RULE_PATH_BENEATH,
            &attr,
            0,
        )
    };
    if r < 0 {
        Err(format!("add_rule: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

/// `PathFd::new(path)` — O_PATH handle for rule attachment.
pub fn ll_path_fd(path: &Path) -> Result<std::os::fd::RawFd, String> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|e| format!("invalid path: {e}"))?;
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
    if fd < 0 {
        Err(format!(
            "PathFd {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(fd)
    }
}

/// `prctl(NO_NEW_PRIVS)` + `landlock_restrict_self` — returns true when the
/// ruleset was fully enforced (mirrors `RulesetStatus::FullyEnforced`).
pub fn ll_restrict_self(ruleset_fd: std::os::fd::RawFd) -> Result<bool, String> {
    // Landlock requires no_new_privs before restrict_self.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(format!(
            "prctl(NO_NEW_PRIVS): {}",
            std::io::Error::last_os_error()
        ));
    }
    let r = unsafe { libc::syscall(libc::SYS_landlock_restrict_self, ruleset_fd, 0) };
    if r < 0 {
        Err(format!(
            "Failed to enforce Landlock sandbox: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(r == 0)
    }
}
