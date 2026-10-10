// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! systemd user unit management for `idle-daemon`.

use std::io;

const UNITS: &[&str] = &["idle-daemon.service", "trance-daemon.service"];

/// Start and enable the user unit.
pub fn start() -> io::Result<bool> {
    for unit in UNITS {
        if super::try_command("systemctl", &["--user", "enable", "--now", unit])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Stop the user unit without disabling it.
pub fn stop() -> io::Result<bool> {
    for unit in UNITS {
        if super::try_command("systemctl", &["--user", "stop", unit])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Restart the user unit.
pub fn restart() -> io::Result<bool> {
    for unit in UNITS {
        if super::try_command("systemctl", &["--user", "restart", unit])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if the unit is active.
pub fn is_active() -> io::Result<bool> {
    for unit in UNITS {
        if super::try_command("systemctl", &["--user", "is-active", "--quiet", unit])? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_contain_idle_daemon() {
        assert!(UNITS.contains(&"idle-daemon.service"));
    }

    #[test]
    fn systemd_calls_return_ok_bool() {
        assert!(start().is_ok());
        assert!(stop().is_ok());
        assert!(restart().is_ok());
        assert!(is_active().is_ok());
    }
}
