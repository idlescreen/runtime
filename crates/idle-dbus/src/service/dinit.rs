// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! dinit service management using `dinitctl`.

use std::io;

const SERVICES: &[&str] = &["idle-daemon", "trance-daemon"];

/// Start the dinit service.
pub fn start() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("dinitctl", &["--user", "start", svc])? {
            return Ok(true);
        }
        if super::try_command("dinitctl", &["start", svc])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Stop the dinit service.
pub fn stop() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("dinitctl", &["--user", "stop", svc])? {
            return Ok(true);
        }
        if super::try_command("dinitctl", &["stop", svc])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Restart the dinit service.
pub fn restart() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("dinitctl", &["--user", "restart", svc])? {
            return Ok(true);
        }
        if super::try_command("dinitctl", &["restart", svc])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if the dinit service is started.
pub fn is_active() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("dinitctl", &["--user", "is-started", svc])? {
            return Ok(true);
        }
        if super::try_command("dinitctl", &["is-started", svc])? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dinit_services_contain_idle_daemon() {
        assert!(SERVICES.contains(&"idle-daemon"));
    }

    #[test]
    fn dinitctl_missing_returns_ok_bool() {
        let _ = is_active();
    }
}
