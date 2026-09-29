// SPDX-License-Identifier: MIT

//! Pidfile acquire/release with `O_NOFOLLOW | O_CREAT | O_EXCL` to close the
//! read-then-write TOCTOU window and refuse symlink redirection by a same-UID
//! attacker that controls `$XDG_RUNTIME_DIR`.

use std::fs;
use std::path::{Path, PathBuf};

fn pid_file_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("idle-daemon.pid")
    } else {
        std::env::temp_dir().join("idle-daemon.pid")
    }
}

fn is_process_idle_daemon(pid: i32) -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) {
            for line in status.lines() {
                if line.starts_with("State:") {
                    let state_str = line.trim_start_matches("State:").trim();
                    if state_str.starts_with('Z')
                        || state_str.starts_with('X')
                        || state_str.starts_with('x')
                        || state_str.starts_with('T')
                        || state_str.starts_with('t')
                    {
                        return false;
                    }
                }
                if line.starts_with("Threads:")
                    && let Ok(threads) = line.trim_start_matches("Threads:").trim().parse::<u32>()
                    && threads == 0
                {
                    return false;
                }
            }
        } else {
            return false;
        }
        if let Ok(wchan) = fs::read_to_string(format!("/proc/{pid}/wchan")) {
            let w = wchan.trim();
            if w == "do_exit"
                || w == "release_task"
                || w == "exit_mm"
                || w == "sys_exit"
                || w == "do_group_exit"
                || w == "sys_exit_group"
            {
                return false;
            }
        }
        if let Ok(cmdline) = fs::read_to_string(format!("/proc/{pid}/cmdline")) {
            if cmdline.is_empty() {
                return false;
            }
            if let Some(argv0) = cmdline.split('\0').next() {
                let path = std::path::Path::new(argv0);
                if path.file_name().and_then(|s| s.to_str()) == Some("idle-daemon") {
                    return true;
                }
            }
        }
        if let Ok(comm) = fs::read_to_string(format!("/proc/{pid}/comm"))
            && comm.trim() == "idle-daemon"
        {
            return true;
        }
        if Path::new(&format!("/proc/{pid}")).exists() {
            return false;
        }
    }
    unsafe { libc::kill(pid, 0) == 0 }
}

pub(crate) fn acquire_pidfile() -> idle_err::Result<Option<PathBuf>> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let path = pid_file_path();
    for attempt in 0..8u32 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(std::process::id().to_string().as_bytes())
                    .with_context(|| format!("writing pid to {}", path.display()))?;
                return Ok(Some(path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // The path exists. With `O_NOFOLLOW` on a symlink, the
                // kernel returns `EEXIST` (not `ELOOP`) because
                // `O_CREAT|O_EXCL` sees the symlink as an existing file.
                // We must refuse symlinks here — they may redirect writes
                // elsewhere — and only operate on a regular file.
                let md = fs::symlink_metadata(&path).map_err(|e| {
                    idle_err::Error::new(e).context(format!("stat pidfile {}", path.display()))
                })?;
                if md.file_type().is_symlink() {
                    idle_err::bail!("refusing to follow symlinked pidfile at {}", path.display());
                }
                let pid = fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| s.trim().parse::<i32>().ok());
                if let Some(pid) = pid {
                    if pid == std::process::id() as i32 {
                        return Ok(Some(path));
                    }
                    // SAFETY: signal 0 only probes existence; no signal is delivered.
                    unsafe {
                        if libc::kill(pid, 0) == 0 {
                            let mut is_active = false;
                            for i in 0..4 {
                                if is_process_idle_daemon(pid) {
                                    is_active = true;
                                    if i < 3 {
                                        std::thread::sleep(std::time::Duration::from_millis(50));
                                    }
                                } else {
                                    is_active = false;
                                    break;
                                }
                            }
                            if is_active {
                                idle_log::warn!(
                                    "idle-daemon is already running (pid {pid}). Exiting."
                                );
                                return Ok(None);
                            }
                            idle_log::warn!(
                                "Overwriting stale PID file (pid {pid} is not idle-daemon)."
                            );
                        }
                    }
                }
                let _ = fs::remove_file(&path);
                std::thread::sleep(std::time::Duration::from_millis(50));
                if attempt == 7 {
                    idle_err::bail!(
                        "could not acquire pid file at {} after {} attempts",
                        path.display(),
                        attempt + 1
                    );
                }
            }
            Err(e) => {
                return Err(idle_err::Error::new(e)
                    .context(format!("creating pid file at {}", path.display())));
            }
        }
    }
    // Unreachable: the for-loop runs at most 8 iterations and either
    // returns Ok(Some(path)) inside the loop or returns Err.
    Err(idle_err::anyhow!(
        "pidfile acquire loop exited without success or error (unreachable)"
    ))
}

pub(crate) fn release_pidfile(path: &Path) {
    let _ = fs::remove_file(path);
}

use idle_err::Context;

#[cfg(test)]
#[path = "pidfile_tests.rs"]
mod pidfile_tests;
