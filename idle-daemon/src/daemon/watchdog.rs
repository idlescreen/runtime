// SPDX-License-Identifier: MIT

//! Render-loop watchdog (Sprint 04 G2).
//!
//! Strategy: the main tick loop records a heartbeat (monotonic millis) after
//! each iteration. A background monitor thread checks the timestamp; if it
//! has not advanced past `timeout_ms`, the monitor reports the loop as
//! stalled and (in Sprint 04) emits an `error!` log + an optional
//! `std::process::exit(1)` trigger.
//!
//! Process-level restart is a follow-up (would require the daemon to spawn a
//! supervisor around itself); tracked as a residual. This primitive gives the
//! host the watchdog *signal* — operator scripts / journald can act on the
//! `idle_log::error!` line.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default heartbeat timeout (no recorded heartbeat in 5s = stalled).
pub const DEFAULT_HEARTBEAT_TIMEOUT_MS: u64 = 5_000;
/// Background monitor poll interval.
pub const DEFAULT_MONITOR_INTERVAL: Duration = Duration::from_millis(500);

/// Shared watchdog primitive. Cloned cheaply; safe to share across threads.
#[derive(Clone)]
pub struct Watchdog {
    last_heartbeat_ms: Arc<AtomicU64>,
}

impl Default for Watchdog {
    fn default() -> Self {
        Self::new()
    }
}

impl Watchdog {
    /// Construct a fresh watchdog. Records `now()` so the first monitor
    /// check has a baseline.
    pub fn new() -> Self {
        let wd = Self {
            last_heartbeat_ms: Arc::new(AtomicU64::new(0)),
        };
        wd.heartbeat();
        wd
    }

    /// Record that the monitored loop made progress. Idempotent.
    pub fn heartbeat(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.last_heartbeat_ms.store(now, Ordering::Release);
    }

    /// Milliseconds since the last heartbeat.
    pub fn age_ms(&self) -> u64 {
        let last = self.last_heartbeat_ms.load(Ordering::Acquire);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        now.saturating_sub(last)
    }

    /// True when no heartbeat has been recorded for `timeout_ms`.
    pub fn stalled(&self, timeout_ms: u64) -> bool {
        self.age_ms() > timeout_ms
    }
}

/// Spawn the monitor thread. Returns the join handle; caller drops it
/// to detach, or `.await`s to stop on shutdown.
///
/// `shutdown_flag` is set to `true` the first time a stall is detected
/// (so the daemon's main loop exits and the process supervisor can
/// restart). The `Watchdog::stalled()` call is repeated every
/// `DEFAULT_MONITOR_INTERVAL`; on the first hit the flag is raised, on
/// subsequent hits the log line is repeated but the flag is not re-toggled.
pub fn spawn_monitor(
    watchdog: Watchdog,
    timeout_ms: u64,
    shutdown_flag: Arc<std::sync::atomic::AtomicBool>,
    stall_flag: Arc<std::sync::atomic::AtomicBool>,
    main_thread: thread::Thread,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut escalated = false;
        loop {
            if watchdog.stalled(timeout_ms) {
                idle_log::error!(
                    age_ms = watchdog.age_ms(),
                    timeout_ms,
                    "render loop stalled — escalating (raising shutdown flag)"
                );
                if !escalated {
                    stall_flag.store(true, std::sync::atomic::Ordering::Release);
                    shutdown_flag.store(true, std::sync::atomic::Ordering::Release);
                    main_thread.unpark();
                    escalated = true;
                }
            }
            thread::sleep(DEFAULT_MONITOR_INTERVAL);
        }
    })
}

/// Read the configured timeout (ms), defaulting to `DEFAULT_HEARTBEAT_TIMEOUT_MS`.
pub fn configured_timeout_ms() -> u64 {
    std::env::var("IDLE_HEARTBEAT_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_HEARTBEAT_TIMEOUT_MS)
}

#[cfg(test)]
#[path = "watchdog_tests.rs"]
mod tests;
