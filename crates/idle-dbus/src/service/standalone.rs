// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Standalone process supervision and PID file management.
//!
//! Spawns `idle-daemon daemon` detached, and validates PID files using
//! `O_NOFOLLOW` and `/proc/<pid>` argv0/comm validation to defend against spoofing.

use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const DAEMON_BINS: &[&str] = &["idle-daemon", "idlescreen-daemon", "trance-daemon"];
const PIDFILES: &[&str] = &[
    "idle-daemon.pid",
    "idlescreen-daemon.pid",
    "trance-daemon.pid",
];

/// Directly spawn `idle-daemon daemon`.
pub fn start() -> io::Result<bool> {
    for bin in DAEMON_BINS {
        if Command::new(bin).arg("daemon").spawn().is_ok() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Stop the daemon by signaling its verified PID from pidfile with SIGTERM.
pub fn stop() -> io::Result<bool> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok();
    for name in PIDFILES {
        let pid_path = match runtime {
            Some(ref dir) => PathBuf::from(dir).join(name),
            None => std::env::temp_dir().join(name),
        };
        let Some(pid) = read_pidfile_safely(&pid_path) else {
            continue;
        };
        if !pid_targets_idle_daemon(pid) {
            idle_log::warn!("refusing to SIGTERM pid {pid} — not idle-daemon");
            continue;
        }
        // SAFETY: SIGTERM sent only to verified idle-daemon process.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        return Ok(true);
    }
    Ok(false)
}

/// Restart the standalone daemon by stopping it, pausing, and restarting.
pub fn restart() -> io::Result<bool> {
    let _ = stop();
    thread::sleep(Duration::from_millis(150));
    start()
}

/// Read a pidfile via `O_NOFOLLOW`. Returns parsed pid if the file is a regular
/// file containing a parseable integer. Symlink paths return `None`.
pub fn read_pidfile_safely(path: &Path) -> Option<i32> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    buf.trim().parse::<i32>().ok()
}

/// Returns true if an argument from `/proc/<pid>/cmdline` represents an
/// idle-daemon executable (either bare name or path suffix).
pub fn is_daemon_cmdline_arg(argv: &str) -> bool {
    argv == "idle-daemon"
        || argv.ends_with("/idle-daemon")
        || argv == "idlescreen-daemon"
        || argv.ends_with("/idlescreen-daemon")
        || argv == "trance-daemon"
        || argv.ends_with("/trance-daemon")
}

/// Returns true if `/proc/<pid>/comm` represents an idle-daemon executable,
/// taking into account Linux `TASK_COMM_LEN` (16 bytes, 15 chars) truncation.
pub fn is_daemon_comm(comm: &str) -> bool {
    let c = comm.trim();
    c == "idle-daemon"
        || c == "idlescreen-daemon"
        || c == "idlescreen-daem"
        || c == "idlescreen-"
        || c == "trance-daemon"
        || c == "trance-"
        || c.starts_with("idlescreen")
}

/// Verify that `/proc/<pid>/cmdline` AND `/proc/<pid>/comm` BOTH identify
/// the target as idle-daemon before signaling it.
pub fn pid_targets_idle_daemon(pid: i32) -> bool {
    let cmdline_match = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .map(|s| {
            let mut args = s.split('\0').filter(|a| !a.is_empty());
            let Some(argv0) = args.next() else {
                return false;
            };
            if is_daemon_cmdline_arg(argv0) {
                return true;
            }
            if (argv0 == "idlescreen" || argv0.ends_with("/idlescreen"))
                && args.any(|a| a == "daemon")
            {
                return true;
            }
            false
        })
        .unwrap_or(false);
    if !cmdline_match {
        return false;
    }
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| is_daemon_comm(&s))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pidfile_not_found_returns_none() {
        let non_existent = Path::new("/tmp/nonexistent-idle-test.pid");
        assert_eq!(read_pidfile_safely(non_existent), None);
    }

    #[test]
    fn pid_targets_idle_daemon_rejects_init_and_self() {
        assert!(!pid_targets_idle_daemon(1));
        assert!(!pid_targets_idle_daemon(std::process::id() as i32));
    }

    #[test]
    fn cmdline_arg_matching_bare_and_paths() {
        assert!(is_daemon_cmdline_arg("idle-daemon"));
        assert!(is_daemon_cmdline_arg("/usr/bin/idle-daemon"));
        assert!(is_daemon_cmdline_arg("./idle-daemon"));
        assert!(is_daemon_cmdline_arg("/opt/idlescreen/bin/idle-daemon"));

        assert!(is_daemon_cmdline_arg("idlescreen-daemon"));
        assert!(is_daemon_cmdline_arg("/usr/local/bin/idlescreen-daemon"));

        assert!(is_daemon_cmdline_arg("trance-daemon"));
        assert!(is_daemon_cmdline_arg("/usr/bin/trance-daemon"));

        assert!(!is_daemon_cmdline_arg("not-idle-daemon"));
        assert!(!is_daemon_cmdline_arg("/usr/bin/my-idle-daemon"));
        assert!(!is_daemon_cmdline_arg("/usr/bin/idle-daemon-helper"));
        assert!(!is_daemon_cmdline_arg("bash"));
        assert!(!is_daemon_cmdline_arg(""));
    }

    #[test]
    fn comm_matching_variants_and_truncations() {
        assert!(is_daemon_comm("idle-daemon"));
        assert!(is_daemon_comm("idle-daemon\n"));
        assert!(is_daemon_comm("trance-daemon"));
        assert!(is_daemon_comm("trance-"));
        assert!(is_daemon_comm("idlescreen-daemon"));
        // Linux TASK_COMM_LEN 15-character truncation check
        assert!(is_daemon_comm("idlescreen-daem"));
        assert!(is_daemon_comm("idlescreen-"));

        assert!(!is_daemon_comm("systemd"));
        assert!(!is_daemon_comm("bash"));
        assert!(!is_daemon_comm("gedit"));
        assert!(!is_daemon_comm("idle"));
        assert!(!is_daemon_comm(""));
    }
}
