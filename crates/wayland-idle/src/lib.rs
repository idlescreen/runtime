// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Wayland idle detection using the [`ext-idle-notify-v1`] protocol.
//!
//! Compositors such as COSMIC, Sway, Hyprland, and KWin expose this extension.
//! When a Wayland session is available, [`IdleMonitor`] connects to the compositor
//! and reports whether the user has been inactive longer than the configured timeout.
//!
//! The Wayland client runs on a background thread (see the `wayland` submodule);
//! the public [`IdleMonitor`] handle is safe to poll from the daemon main loop.
//!
//! [`ext-idle-notify-v1`]: https://wayland.app/protocols/ext-idle-notify-v1
//!
//! If the extension is unavailable, [`IdleMonitor::new`] returns `None` and the
//! daemon should refuse to start rather than falling back to X11 screensaver APIs.

mod monitor;
mod wayland;

pub use monitor::IdleMonitor;

// Timeout changes are forwarded to the Wayland thread without reconnecting.
// Requires WAYLAND_DISPLAY in the daemon environment.
// Polling is cheap: state is mirrored with atomics from the Wayland thread.
// No X11 ScreenSaver extension fallback is attempted in this crate.

#[cfg(test)]
pub(crate) static TEST_MUTEX: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) fn get_test_mutex() -> &'static std::sync::Mutex<()> {
    TEST_MUTEX.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_availability_and_fallback() {
        let _lock = crate::get_test_mutex().lock().unwrap();
        let backup = std::env::var("WAYLAND_DISPLAY").ok();

        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
        }
        assert!(!IdleMonitor::is_available());
        assert!(IdleMonitor::new(10).is_none());

        unsafe {
            std::env::set_var("WAYLAND_DISPLAY", "wayland-mock-test-0");
        }
        assert!(IdleMonitor::is_available());

        if let Some(val) = backup {
            unsafe {
                std::env::set_var("WAYLAND_DISPLAY", val);
            }
        } else {
            unsafe {
                std::env::remove_var("WAYLAND_DISPLAY");
            }
        }
    }
}
