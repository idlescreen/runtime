// SPDX-License-Identifier: MIT

//! cgroup v2 plumbing for the plugin CPU/memory budget — root detection,
//! child creation, `cpu.max`/`memory.max` writes, usage reads.

use std::io;
use std::path::{Path, PathBuf};

/// Detect cgroup v2 root. `/sys/fs/cgroup/cgroup.controllers` is the v2 marker.
pub(crate) fn cgroup_v2_root() -> Option<PathBuf> {
    let p = PathBuf::from("/sys/fs/cgroup");
    if p.join("cgroup.controllers").exists() {
        Some(p)
    } else {
        None
    }
}

/// Try to mkdir the child, write `cpu.max`, and attach the current thread.
/// Any failure (no v2, no write perm, etc.) bubbles up so the caller falls
/// back to in-process measurement only.
pub(crate) fn try_attach_cgroup(
    plugin_id: &str,
    quota_us: u64,
    period_us: u64,
) -> io::Result<PathBuf> {
    let root = cgroup_v2_root()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cgroup v2 not mounted"))?;
    let dir = root.join("idle").join(plugin_id);
    std::fs::create_dir_all(&dir)?;
    // cpu.max format: "<quota> <period>" — "max <period>" disables the cap.
    std::fs::write(dir.join("cpu.max"), format!("{quota_us} {period_us}"))?;
    // Memory cap: best-effort — the memory controller is not always delegated
    // to user cgroups. A runaway saver otherwise OOMs the runner; here the
    // kernel kills only this cgroup's members.
    let mem_bytes = std::env::var("IDLE_RUNNER_MEM_MB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(2048)
        * 1024
        * 1024;
    if let Err(e) = std::fs::write(dir.join("memory.max"), mem_bytes.to_string()) {
        idle_log::debug!("memory.max write skipped (controller not delegated?): {e}");
    }
    // Attach the current thread (id matches cgroup.procs; thread-id is valid
    // when cgroup v2 is enabled with `cgroup.threads`).
    let tid = format!("{}", unsafe { libc::syscall(libc::SYS_gettid) });
    std::fs::write(dir.join("cgroup.threads"), tid.as_bytes())?;
    // Also attach the process so the worker thread inherits the budget.
    let pid = format!("{}", std::process::id());
    std::fs::write(dir.join("cgroup.procs"), pid.as_bytes())?;
    Ok(dir)
}

pub(crate) fn read_cgroup_usage_micros(dir: &Path) -> io::Result<u64> {
    // cpu.stat format: key value lines; we want `usage_usec <N>`.
    let text = std::fs::read_to_string(dir.join("cpu.stat"))?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("usage_usec ") {
            return rest.trim().parse::<u64>().map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("usage_usec parse: {e}"))
            });
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "cpu.stat missing usage_usec",
    ))
}
