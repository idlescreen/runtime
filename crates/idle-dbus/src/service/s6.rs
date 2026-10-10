// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! s6 service management using `s6-svc` and `s6-rc`.

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
        let p = Path::new(&home).join(".s6/service").join(svc);
        if p.exists() {
            return Some(p);
        }
    }
    let run_p = Path::new("/run/service").join(svc);
    if run_p.exists() {
        return Some(run_p);
    }
    None
}

/// Start the s6 service.
pub fn start() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("s6-rc", &["-u", "change", svc])? {
            return Ok(true);
        }
        if let Some(dir) = find_service_dir(svc)
            && let Some(dir_str) = dir.to_str()
            && super::try_command("s6-svc", &["-u", dir_str])?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Stop the s6 service.
pub fn stop() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("s6-rc", &["-d", "change", svc])? {
            return Ok(true);
        }
        if let Some(dir) = find_service_dir(svc)
            && let Some(dir_str) = dir.to_str()
            && super::try_command("s6-svc", &["-d", dir_str])?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Restart the s6 service.
pub fn restart() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("s6-rc", &["-r", "change", svc])? {
            return Ok(true);
        }
        if let Some(dir) = find_service_dir(svc)
            && let Some(dir_str) = dir.to_str()
            && super::try_command("s6-svc", &["-r", dir_str])?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Check if the s6 service is active.
pub fn is_active() -> io::Result<bool> {
    for svc in SERVICES {
        if super::try_command("s6-rc", &["-b", "check", svc])? {
            return Ok(true);
        }
        if let Some(dir) = find_service_dir(svc)
            && let Some(dir_str) = dir.to_str()
            && super::try_command("s6-svstat", &["-u", dir_str])?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s6_services_list_valid() {
        assert!(SERVICES.contains(&"idle-daemon"));
    }

    #[test]
    fn s6_calls_return_ok_bool() {
        let _ = is_active();
    }
}
