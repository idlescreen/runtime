// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Modular service lifecycle management for `idle-daemon`.
//!
//! Dispatches start, stop, and restart commands to the detected init system
//! (systemd, OpenRC, runit, dinit, s6, or standalone PID supervision).

use std::io;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::daemon_available;

pub mod detect;
pub mod dinit;
pub mod openrc;
pub mod runit;
pub mod s6;
pub mod standalone;
pub mod systemd;

pub use detect::{InitSystem, detect_init_system};

/// Execute a command safely without aborting on missing supervisor binaries.
/// Returns `Ok(true)` on exit 0, `Ok(false)` if missing or non-zero, never aborts.
pub(crate) fn try_command(cmd: &str, args: &[&str]) -> io::Result<bool> {
    match Command::new(cmd).args(args).status() {
        Ok(status) => Ok(status.success()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => {
            idle_log::warn!("error running {cmd}: {e}");
            Ok(false)
        }
    }
}

/// Start the daemon service via the detected init system, falling back
/// to direct standalone spawn if supervisor fails or is uninstalled.
pub fn start_daemon_service() -> io::Result<()> {
    let init = detect_init_system();
    let started = match init {
        InitSystem::Systemd => systemd::start()?,
        InitSystem::OpenRc => openrc::start()?,
        InitSystem::Runit => runit::start()?,
        InitSystem::Dinit => dinit::start()?,
        InitSystem::S6 => s6::start()?,
        InitSystem::Standalone => standalone::start()?,
    };

    if started {
        return wait_until_running(Duration::from_secs(3));
    }

    // Graceful fallback to standalone direct spawn if supervisor failed
    if init != InitSystem::Standalone {
        idle_log::warn!("{init} start failed; falling back to direct spawn");
        if standalone::start()? {
            return wait_until_running(Duration::from_secs(3));
        }
    }

    Err(io::Error::other(
        "could not start idle-daemon via init supervisor or direct spawn",
    ))
}

/// Stop the daemon service via the detected init system, falling back
/// to PID-file SIGTERM if supervisor is inactive or unmanaged.
pub fn stop_daemon_service() -> io::Result<()> {
    let init = detect_init_system();
    let stopped = match init {
        InitSystem::Systemd => systemd::stop()?,
        InitSystem::OpenRc => openrc::stop()?,
        InitSystem::Runit => runit::stop()?,
        InitSystem::Dinit => dinit::stop()?,
        InitSystem::S6 => s6::stop()?,
        InitSystem::Standalone => standalone::stop()?,
    };

    if stopped {
        return Ok(());
    }

    // Graceful fallback to PID-file kill if supervisor stop was ineffective
    if init != InitSystem::Standalone {
        idle_log::warn!("{init} stop failed; falling back to PID file termination");
        if standalone::stop()? {
            return Ok(());
        }
    }

    Err(io::Error::other(
        "could not stop idle-daemon via init supervisor or PID file",
    ))
}

/// Restart the daemon service via the detected init system, falling back
/// to standalone restart if supervisor restart fails.
pub fn restart_daemon_service() -> io::Result<()> {
    let init = detect_init_system();
    let restarted = match init {
        InitSystem::Systemd => systemd::restart()?,
        InitSystem::OpenRc => openrc::restart()?,
        InitSystem::Runit => runit::restart()?,
        InitSystem::Dinit => dinit::restart()?,
        InitSystem::S6 => s6::restart()?,
        InitSystem::Standalone => standalone::restart()?,
    };

    if restarted {
        return wait_until_running(Duration::from_secs(3));
    }

    if init != InitSystem::Standalone {
        idle_log::warn!("{init} restart failed; falling back to standalone restart");
        if standalone::restart()? {
            return wait_until_running(Duration::from_secs(3));
        }
    }

    Err(io::Error::other(
        "could not restart idle-daemon via init supervisor or standalone fallback",
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
            "idle-daemon did not become reachable on session bus within {budget:?}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_command_missing_binary_returns_false_not_err() {
        let res = try_command("definitely-nonexistent-supervisor-binary-xyz", &[]);
        assert!(!res.unwrap());
    }

    #[test]
    fn try_command_successful_binary() {
        let res = try_command("sh", &["-c", "exit 0"]);
        assert!(res.unwrap());
    }

    #[test]
    fn try_command_failing_binary() {
        let res = try_command("sh", &["-c", "exit 1"]);
        assert!(!res.unwrap());
    }

    #[test]
    fn error_kind_is_other_not_not_found() {
        let err = io::Error::other("could not start idle-daemon");
        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_ne!(err.kind(), io::ErrorKind::NotFound);
    }
}
