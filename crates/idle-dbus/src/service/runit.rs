// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! runit service management using `sv`.

use std::io;
use std::path::{Path, PathBuf};

const SERVICES: &[&str] = &["idle-daemon", "trance-daemon"];

fn find_service_dir(svc: &str) -> Option<PathBuf> {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        let p = Path::new(&runtime).join("service").join(svc);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = Path::new(&home).join("service").join(svc);
        if p.exists() {
            return Some(p);
        }
    }
    let var_p = Path::new("/var/service").join(svc);
    if var_p.exists() {
        return Some(var_p);
    }
    None
}

/// Start the runit service.
pub fn start() -> io::Result<bool> {
    for svc in SERVICES {
        if let Some(dir) = find_service_dir(svc)
            && let Some(dir_str) = dir.to_str()
            && super::try_command("sv", &["start", dir_str])?
        {
            return Ok(true);
        }
        if super::try_command("sv", &["start", svc])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Stop the runit service.
pub fn stop() -> io::Result<bool> {
    for svc in SERVICES {
        if let Some(dir) = find_service_dir(svc)
            && let Some(dir_str) = dir.to_str()
            && super::try_command("sv", &["stop", dir_str])?
        {
            return Ok(true);
        }
        if super::try_command("sv", &["stop", svc])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Restart the runit service.
pub fn restart() -> io::Result<bool> {
    for svc in SERVICES {
        if let Some(dir) = find_service_dir(svc)
            && let Some(dir_str) = dir.to_str()
            && super::try_command("sv", &["restart", dir_str])?
        {
            return Ok(true);
        }
        if super::try_command("sv", &["restart", svc])? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if the runit service is active.
pub fn is_active() -> io::Result<bool> {
    for svc in SERVICES {
        if let Some(dir) = find_service_dir(svc)
            && let Some(dir_str) = dir.to_str()
            && super::try_command("sv", &["status", dir_str])?
        {
            return Ok(true);
        }
        if super::try_command("sv", &["status", svc])? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_list_valid() {
        assert!(SERVICES.contains(&"idle-daemon"));
    }

    #[test]
    fn runit_calls_do_not_error() {
        let _ = is_active();
    }
}
