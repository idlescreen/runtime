// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Platform-agnostic idle-detection contract (Sprint 04 G3/G4).
//!
//! Linux today ships a Wayland `ext-idle-notify-v1` impl via the
//! `wayland-idle` crate; macOS / Windows are stubbed and return `None`
//! until Sprint 05 lands the real `IOKit` / `GetLastInputInfo` shims.
//!
//! Every impl must fail closed: `is_available()` returning false or
//! `new()` returning `None` is the signal to the daemon to refuse to
//! start rather than fall back to a less-secure idle source.

use std::time::Duration;

/// Idle-time contract every platform implementation satisfies.
///
/// `new` returns `None` when the platform's idle source is unavailable
/// (no Wayland compositor, missing IOKit, etc.). Callers must treat
/// `None` as a hard refusal — not fall back to polling input devices
/// directly, which would be both unreliable and a sandbox-bypass.
pub trait IdleSource: Send + 'static {
    /// True when the implementation can connect to its platform idle source
    /// in this environment (e.g. `WAYLAND_DISPLAY` is set on Linux).
    fn is_available() -> bool
    where
        Self: Sized;

    /// Connect to the platform idle source and start polling.
    ///
    /// Returns `None` when the source is unavailable. `timeout` is the
    /// initial inactivity threshold; the platform reports `is_idle()`
    /// once the user has been inactive for at least that duration.
    fn new(timeout: Duration) -> Option<Self>
    where
        Self: Sized;

    /// True when the user has been idle longer than the configured threshold.
    fn is_idle(&self) -> bool;

    /// True when the underlying event monitoring thread is still running.
    /// Returns `false` to signal the daemon that the source is wedged
    /// and the host needs to be restarted.
    fn is_alive(&self) -> bool;

    /// Update the inactivity threshold without reconnecting.
    fn set_timeout(&self, timeout: Duration);
}

/// Select the idle source for the current target OS at compile time.
///
/// Returns `None` when the target has no impl yet (Sprint 05 territory).
/// The daemon should log a clear error and refuse to start.
#[cfg(target_os = "linux")]
pub fn platform_idle(timeout: Duration) -> Option<Box<dyn IdleSource>> {
    // Linux: the `wayland-idle` crate is the canonical impl. We avoid
    // a hard dep here so `idle-api` does not pull `wayland-client`;
    // callers on Linux pass their own `Box<dyn IdleSource>` from the
    // `wayland-idle` crate. This function therefore returns `None`
    // unconditionally on Linux; the runner's Linux path constructs
    // the source directly. On other targets the stub below applies.
    let _ = timeout;
    None
}

#[cfg(not(target_os = "linux"))]
pub fn platform_idle(timeout: Duration) -> Option<Box<dyn IdleSource>> {
    StubIdleSource::new(timeout).map(|s| Box::new(s) as Box<dyn IdleSource>)
}

/// Stub impl for non-Linux targets. Always returns "not available"
/// until Sprint 05 wires real IOKit / GetLastInputInfo impls. The
/// constructor never errors; it returns a handle that reports
/// `is_idle() == false` and `is_alive() == false`, so the daemon
/// observes the failure immediately rather than spinning.
pub struct StubIdleSource {
    timeout: Duration,
}

impl IdleSource for StubIdleSource {
    fn is_available() -> bool {
        false
    }

    fn new(timeout: Duration) -> Option<Self> {
        Some(Self { timeout })
    }

    fn is_idle(&self) -> bool {
        // Always false — the daemon must not present a screensaver when
        // there is no live idle source. Pair with `is_alive() == false`
        // to fail fast.
        let _ = self.timeout;
        false
    }

    fn is_alive(&self) -> bool {
        false
    }

    fn set_timeout(&self, timeout: Duration) {
        // No-op; the stub can't reach a platform source. Real impls
        // forward the timeout to the platform event loop.
        let _ = timeout;
    }
}

#[cfg(test)]
#[path = "idle_source_tests.rs"]
mod tests;
