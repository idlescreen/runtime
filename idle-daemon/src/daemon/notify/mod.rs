// SPDX-License-Identifier: MIT

//! Multi-init readiness notification facade.
//!
//! Coordinates readiness signaling across:
//! 1. D-Bus session name acquisition verification (OpenRC & standalone)
//! 2. Systemd `NOTIFY_SOCKET` datagrams with abstract socket support
//! 3. File descriptor pipe readiness (fd 3) for s6 and dinit

pub mod bus;
pub mod fd_pipe;
pub mod systemd;

#[cfg(test)]
mod tests;

use std::time::Duration;

/// Default timeout to verify D-Bus bus-name acquisition during readiness notification.
const DEFAULT_BUS_WAIT: Duration = Duration::from_millis(500);

/// Emit readiness notifications across all supported init channels.
pub fn notify_ready() {
    if let Err(err) = systemd::notify_ready() {
        idle_log::debug!("systemd notify_ready: {err}");
    }

    if let Err(err) = fd_pipe::notify_ready() {
        idle_log::debug!("fd_pipe notify_ready: {err}");
    }

    let _ = bus::verify_bus_readiness(DEFAULT_BUS_WAIT);
}

/// Emit stopping notification to supervisor.
pub fn notify_stopping() {
    if let Err(err) = systemd::notify_stopping() {
        idle_log::debug!("systemd notify_stopping: {err}");
    }
}

/// Emit watchdog heartbeat to supervisor.
pub fn notify_watchdog() {
    if let Err(err) = systemd::notify_watchdog() {
        idle_log::debug!("systemd notify_watchdog: {err}");
    }
}
