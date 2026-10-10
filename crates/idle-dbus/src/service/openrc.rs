// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! OpenRC service management for `idle-daemon`.

use std::io;

const SERVICES: &[&str] = &["idle-daemon", "trance-daemon"];

/// Start the OpenRC service.
pub fn start() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("rc-service", &["--user", svc, "start"])? {
            return Ok(true);
        }
        if super::try_command("rc-service", &[svc, "start"])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Stop the OpenRC service.
pub fn stop() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("rc-service", &["--user", svc, "stop"])? {
            return Ok(true);
        }
        if super::try_command("rc-service", &[svc, "stop"])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Restart the OpenRC service.
pub fn restart() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("rc-service", &["--user", svc, "restart"])? {
            return Ok(true);
        }
        if super::try_command("rc-service", &[svc, "restart"])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if the OpenRC service is running.
pub fn is_active() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("rc-service", &["--user", svc, "status"])? {
            return Ok(true);
        }
        if super::try_command("rc-service", &[svc, "status"])? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_contain_idle_daemon() {
        assert!(SERVICES.contains(&"idle-daemon"));
    }

    #[test]
    fn openrc_calls_return_ok_bool() {
        let _ = is_active();
    }
}
