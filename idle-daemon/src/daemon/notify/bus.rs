// SPDX-License-Identifier: MIT

//! D-Bus session bus-name claiming verification.
//!
//! Confirms that `io.github.idlescreen.Idle` has been claimed and registered
//! on the session bus before signaling readiness for OpenRC and standalone.

use std::time::{Duration, Instant};

/// Probe whether the daemon's well-known bus name is currently active.
#[must_use]
pub fn is_bus_name_claimed() -> bool {
    idle_dbus::daemon_available()
}

/// Wait up to `timeout` for the bus name to appear on the session bus.
#[must_use]
pub fn wait_for_bus_name_claimed(timeout: Duration) -> bool {
    if is_bus_name_claimed() {
        return true;
    }

    let start = Instant::now();
    let poll_interval = Duration::from_millis(15);

    while start.elapsed() < timeout {
        std::thread::sleep(poll_interval);
        if is_bus_name_claimed() {
            return true;
        }
    }

    false
}

/// Verify that the session bus is active and the daemon's name is claimed.
///
/// Skips verification when `DBUS_SESSION_BUS_ADDRESS` is not configured.
pub fn verify_bus_readiness(timeout: Duration) -> bool {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        idle_log::debug!("DBUS_SESSION_BUS_ADDRESS unset; skipping bus verification");
        return false;
    }

    let claimed = wait_for_bus_name_claimed(timeout);
    if claimed {
        idle_log::info!(
            "session bus-name {} confirmed active",
            idle_dbus::SERVICE_NAME
        );
    } else {
        idle_log::warn!(
            "timed out waiting for session bus-name {}",
            idle_dbus::SERVICE_NAME
        );
    }
    claimed
}
