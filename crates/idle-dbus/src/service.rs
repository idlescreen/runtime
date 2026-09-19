// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! systemd-user service management for `idle-daemon`, shared by the CLI,
//! TUI, and COSMIC applet so each consumer stops re-implementing the
//! enable/start/stop/wait chain.

use std::io;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::daemon_available;

/// User units tried in order; `trance-daemon.service` is the legacy name.
const UNITS: &[&str] = &["idle-daemon.service", "trance-daemon.service"];
/// Direct-spawn binaries tried when systemctl is unusable.
const DAEMON_BINS: &[&str] = &["idle-daemon", "idlescreen-daemon", "trance-daemon"];
/// Pidfile names under `$XDG_RUNTIME_DIR` (fallback: temp dir).
const PIDFILES: &[&str] = &["idle-daemon.pid", "trance-daemon.pid"];

/// Start the user unit and enable it so it returns after login/upgrades.
///
/// Falls back to spawning `idle-daemon daemon` only if systemctl is
/// unusable (unusual on a session with systemd --user).
pub fn start_daemon_service() -> io::Result<()> {
    for unit in UNITS {
        let status = Command::new("systemctl")
            .args(["--user", "enable", "--now", unit])
            .status()?;

        if status.success() {
            wait_until_running(Duration::from_secs(3))?;
            return Ok(());
        }
        idle_log::warn!(
            "systemctl enable --now {unit} failed (exit {:?})",
            status.code()
        );
    }

    idle_log::warn!("systemctl enable --now failed; trying direct spawn");
    for bin in DAEMON_BINS {
        if Command::new(bin).arg("daemon").spawn().is_ok() {
            wait_until_running(Duration::from_secs(3))?;
            return Ok(());
        }
    }
    Err(io::Error::other(
        "could not start idle-daemon via systemctl or direct spawn",
    ))
}

/// Stop the running user unit (does **not** disable — keeps login autostart).
pub fn stop_daemon_service() -> io::Result<()> {
    for unit in UNITS {
        let status = Command::new("systemctl")
            .args(["--user", "stop", unit])
            .status()?;

        if status.success() {
            return Ok(());
        }
    }

    // Fallback: SIGTERM via PID file if the unit is unmanaged.
    // Read with O_NOFOLLOW so a planted symlink cannot redirect us at a
    // different process, and refuse to signal a pid whose argv0/comm do
    // not identify idle-daemon (defense vs pidfile tampering).
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok();
    for name in PIDFILES {
        let pid_path = match runtime {
            Some(ref dir) => std::path::PathBuf::from(dir).join(name),
            None => std::env::temp_dir().join(name),
        };
        let Some(pid) = read_pidfile_safely(&pid_path) else {
            continue;
        };
        if !pid_targets_idle_daemon(pid) {
            idle_log::warn!("refusing to SIGTERM pid {pid} — not idle-daemon");
            continue;
        }
        // SAFETY: kill with SIGTERM on a process we verified is idle-daemon.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        return Ok(());
    }

    Err(io::Error::other(
        "could not stop idle-daemon via systemctl or PID file",
    ))
}

/// Poll the session bus until the daemon name is owned or `budget` lapses.
pub fn wait_until_running(budget: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + budget;
    while std::time::Instant::now() < deadline {
        if daemon_available() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    if daemon_available() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "idle-daemon did not become reachable on the session bus within {budget:?}"
        )))
    }
}

/// Read a pidfile via `O_NOFOLLOW`. Returns the parsed pid if the file is a
/// regular file containing a parseable integer. Symlink paths return `None`.
fn read_pidfile_safely(path: &std::path::Path) -> Option<i32> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    buf.trim().parse::<i32>().ok()
}

/// Verify that `/proc/<pid>/cmdline` AND `/proc/<pid>/comm` BOTH identify
/// the target as idle-daemon before signaling it. Belt + suspenders:
/// `cmdline` comes from execve's path (hard to fake); `comm` is
/// target-settable via prctl, so it is only a secondary check — comm alone
/// is never accepted (F-203: a prctl(PR_SET_NAME) spoof must fail).
fn pid_targets_idle_daemon(pid: i32) -> bool {
    let cmdline_match = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .map(|s| {
            s.split('\0')
                .filter(|a| !a.is_empty())
                .any(|argv| argv.ends_with("/idle-daemon") || argv.ends_with("/idlescreen-daemon"))
        })
        .unwrap_or(false);
    if !cmdline_match {
        return false;
    }
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| {
            let c = s.trim();
            c == "idle-daemon" || c == "idlescreen-" || c == "idlescreen-daemon"
        })
        .unwrap_or(false)
}
