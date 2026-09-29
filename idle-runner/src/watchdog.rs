// SPDX-License-Identifier: MIT

//! Per-plugin watchdog (Sprint 03 C).
//!
//! Layered protection for the plugin tick:
//! - CPU budget (B) throttles the kernel-side; long-term overage drops the
//!   session.
//! - This watchdog measures wall-clock of each plugin call; on overflow it
//!   drops the session immediately rather than waiting for the budget window.
//! - A background monitor can also enforce an absolute deadline.
//!
//! What this does **not** catch: a plugin whose `update()` spins forever in
//! our own thread (wall-clock is measured *around* the call). That case is
//! bounded by the CPU budget hard ceiling + kernel cgroup throttle, so the
//! worst case is one budget window of runaway CPU before the session is
//! dropped. Process-level isolation (run the plugin in a child) is the
//! follow-up that would catch infinite loops in zero time; tracked as a
//! residual because the in-process libloading model cannot deliver it
//! cheaply.

use std::time::{Duration, Instant};

/// Default per-tick wall-clock ceiling. A plugin whose update/draw takes
/// longer than this is dropped without further work.
pub const DEFAULT_WATCHDOG_TIMEOUT: Duration = Duration::from_millis(250);

/// Per-call wall-clock guard. Constructed once per call site; `check()` after
/// the call returns `Overflow(_)` if the call exceeded the configured budget.
pub struct CallGuard {
    budget: Duration,
    started: Instant,
}

impl CallGuard {
    pub fn new(budget: Duration) -> Self {
        Self {
            budget,
            started: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// True when the elapsed time exceeded the budget.
    pub fn overflowed(&self) -> bool {
        self.started.elapsed() > self.budget
    }
}

/// Resolve the configured watchdog timeout. Operators may tighten with
/// `IDLE_WATCHDOG_TIMEOUT_MS=<n>`; the default is `DEFAULT_WATCHDOG_TIMEOUT`.
pub fn watchdog_timeout() -> Duration {
    std::env::var("IDLE_WATCHDOG_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_WATCHDOG_TIMEOUT)
}

#[cfg(test)]
#[path = "watchdog_tests.rs"]
mod tests;
