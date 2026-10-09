// SPDX-License-Identifier: MIT

use super::*;
use std::time::Duration;

#[test]
fn test_is_available_matches_wayland() {
    let _guard = crate::TEST_ENV_LOCK.lock().unwrap();
    let backup = std::env::var("WAYLAND_DISPLAY").ok();
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", "wayland-mock-test");
    }
    assert!(GnomeIdleMonitor::is_available());
    unsafe {
        std::env::remove_var("WAYLAND_DISPLAY");
    }
    assert!(!GnomeIdleMonitor::is_available());
    if let Some(val) = backup {
        unsafe {
            std::env::set_var("WAYLAND_DISPLAY", val);
        }
    }
}

#[test]
fn test_unavailable_when_wayland_unset() {
    let _guard = crate::TEST_ENV_LOCK.lock().unwrap();
    let backup = std::env::var("WAYLAND_DISPLAY").ok();
    unsafe {
        std::env::remove_var("WAYLAND_DISPLAY");
    }
    let monitor = GnomeIdleMonitor::new(Duration::from_secs(60));
    assert!(monitor.is_none());
    if let Some(val) = backup {
        unsafe {
            std::env::set_var("WAYLAND_DISPLAY", val);
        }
    }
}

#[test]
fn test_returns_none_when_mutter_absent() {
    let _guard = crate::TEST_ENV_LOCK.lock().unwrap();
    let backup = std::env::var("WAYLAND_DISPLAY").ok();
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", "wayland-mock-nonexistent");
    }
    let monitor = GnomeIdleMonitor::new(Duration::from_secs(60));
    assert!(monitor.is_none());
    if let Some(val) = backup {
        unsafe {
            std::env::set_var("WAYLAND_DISPLAY", val);
        }
    }
}
