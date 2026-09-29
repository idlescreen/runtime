// SPDX-License-Identifier: MIT

//! Event-driven battery / AC watcher.
//!
//! The OODA main tick loop used to poll `/sys/class/power_supply` on every
//! iteration (`daemon::battery::is_on_battery`), reading every supply entry's
//! `type`, `online`, and `status` files via `std::fs::read_to_string`. At
//! `MAIN_LOOP_INTERVAL = 250ms` that was ~16 blocking fs syscalls per second
//! on the daemon's main thread — measurable idle CPU on a laptop that's
//! "doing nothing" with the screensaver running.
//!
//! `PowerWatcher` replaces the polling path with an inotify watcher on
//! `/sys/class/power_supply` plus a 1-second heartbeat:
//!
//!   * inotify wakes the watcher thread on any AC-plug / unplug or
//!     battery-status change. The watcher re-reads the supply dirs, updates
//!     `cached_state()`, and notifies the `Condvar`.
//!   * Tick-loop consumer waits on the Condvar with a 1-second timeout so
//!     the watchdog stays live even when nothing changes.
//!
//! If inotify isn't supported at runtime (no `/sys/class/power_supply`,
//! permission denied, etc.) the watcher falls back to "never notify", and
//! the cached value lives forever. That's fine — the daemon already runs on
//! `is_on_battery` results that match the boot-time sample, which is the
//! only sane behavior when the source dir is missing.
//!
//! Design notes:
//!
//!   * `cached_is_on_battery()` reads from an atomic — no Mutex on the
//!     hot path. The Mutex only guards the notify state (last_event_count)
//!     and is dropped before the condvar wake.
//!   * `wait_for_heartbeat()` is the consumer side: predicate-based wait
//!     that returns `WaitOutcome::Notified` on a real event,
//!     `WaitOutcome::Heartbeat` on the 1-second timeout, and never blocks
//!     past the supplied stop-flag flip.
//!   * Cache invalidation uses a generation counter so spurious notifies
//!     from udev-induced churn don't produce redundant OODA re-reads.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

/// 1-second heartbeat: long enough to drive the watchdog on machines with
/// no power events, short enough to keep OODA responsive at idle.
pub const HEARTBEAT: Duration = Duration::from_secs(1);

/// Outcome of [`PowerWatcher::wait_for_heartbeat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// A power-supply event fired while waiting. Consumer should refresh
    /// derived state (cached `is_on_battery`, etc.) before stepping the
    /// OODA loop.
    Notified,
    /// Heartbeat timer elapsed with no event. Watchdog path: heartbeat
    /// the supervisor and continue.
    Heartbeat,
    /// Shutdown flag flipped. Caller should stop polling.
    Stopped,
}

/// Shared, thread-safe handle. Cloning is cheap (Arc bumps).
#[derive(Clone)]
pub struct PowerWatcher {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    /// Atomic snapshot of `is_on_battery()`. Updated by the watcher thread;
    /// read by every OODA tick without taking the lock.
    pub(crate) cached_on_battery: AtomicBool,
    /// Generation counter — increments on every notify. Lets the consumer
    /// distinguish "wakeup was a real event" from "spurious condvar wake".
    pub(crate) notify_count: AtomicU64,
    /// Mutex guarding the condvar predicate. parking_lot so we don't
    /// allocate Guard objects or take the slow path on a clean release.
    pub(crate) predicate_lock: Mutex<Predicate>,
    pub(crate) condvar: Condvar,
}

pub(crate) struct Predicate {
    pub(crate) notify_count: u64,
}

impl PowerWatcher {
    /// Construct a new watcher handle initialized with an explicit battery state.
    pub(crate) fn from_initial_state(on_battery: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                cached_on_battery: AtomicBool::new(on_battery),
                notify_count: AtomicU64::new(0),
                predicate_lock: Mutex::new(Predicate { notify_count: 0 }),
                condvar: Condvar::new(),
            }),
        }
    }

    /// Notify that the power state has updated.
    pub(crate) fn notify_update(&self, on_battery: bool) {
        self.inner
            .cached_on_battery
            .store(on_battery, Ordering::Release);
        let next = self.inner.notify_count.fetch_add(1, Ordering::AcqRel) + 1;
        {
            let mut guard = self.inner.predicate_lock.lock();
            guard.notify_count = next;
        }
        self.inner.condvar.notify_all();
    }

    /// Cached, atomic snapshot of `battery::is_on_battery()`. Held here
    /// so consumers can read it via a single atomic load once the
    /// watcher handle is plumbed through to OODA. The field is updated
    /// by the watcher thread on every inotify event; this read stays
    /// on the hot path with no Mutex acquisition.
    #[allow(dead_code)]
    pub fn cached_is_on_battery(&self) -> bool {
        self.inner.cached_on_battery.load(Ordering::Relaxed)
    }

    /// Last observed notify generation. Bumped on every inotify event the
    /// watcher processes (after re-reading the supply dirs).
    pub fn notify_generation(&self) -> u64 {
        self.inner.notify_count.load(Ordering::Acquire)
    }

    /// Block up to `max` waiting for either an event or the heartbeat.
    /// Returns the moment-class outcome. On `Heartbeat`, call watchdog
    /// heartbeat + continue; on `Notified`, refresh downstream caches
    /// then continue; on `Stopped`, exit.
    pub fn wait_for_heartbeat(&self, stop: &AtomicBool, max: Duration) -> WaitOutcome {
        let deadline = Instant::now() + max;
        // Fast path: shutdown already requested before we entered.
        if stop.load(Ordering::Relaxed) {
            return WaitOutcome::Stopped;
        }
        // Snapshot the generation we last processed. Wait until it
        // advances or the deadline elapses or `stop` flips.
        let observed_generation = self.inner.notify_count.load(Ordering::Acquire);
        let mut guard = self.inner.predicate_lock.lock();
        loop {
            if stop.load(Ordering::Relaxed) {
                return WaitOutcome::Stopped;
            }
            let now = Instant::now();
            if now >= deadline {
                return WaitOutcome::Heartbeat;
            }
            // Wake source: notify generation advanced.
            if guard.notify_count != observed_generation {
                return WaitOutcome::Notified;
            }
            let remaining = deadline.saturating_duration_since(now);
            // `wait_while_for` is parking_lot 0.12's predicate+timeout
            // wait. The closure re-checks the predicate under the lock
            // on every spurious / notify wakeup. We exit the loop when
            // the predicate is false (notify generation moved OR stop
            // was flipped) OR when `remaining` elapses.
            self.inner.condvar.wait_while_for(
                &mut guard,
                |p| !stop.load(Ordering::Relaxed) && p.notify_count == observed_generation,
                remaining,
            );
        }
    }
}

// Measurement seam, re-exported to `daemon/mod.rs::bench_exports` for
// the `draw_frame` bench target.
#[doc(hidden)]
pub mod bench_exports {
    pub use super::{PowerWatcher, WaitOutcome};

    /// Bench-only handle. The production handle is built in
    /// `power_thread` from a live inotify fd; this constructs the same
    /// `Inner` with no I/O so the `wait_for_heartbeat` lock/condvar
    /// path can be measured in isolation.
    pub fn new_bench_handle() -> PowerWatcher {
        PowerWatcher::from_initial_state(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_returns_stopped_when_stop_already_set() {
        let watcher = PowerWatcher::from_initial_state(false);
        let stop = AtomicBool::new(true);
        let outcome = watcher.wait_for_heartbeat(&stop, HEARTBEAT);
        assert_eq!(outcome, WaitOutcome::Stopped);
    }

    #[test]
    fn wait_returns_heartbeat_on_deadline() {
        let watcher = PowerWatcher::from_initial_state(false);
        let stop = AtomicBool::new(false);
        let start = Instant::now();
        let outcome = watcher.wait_for_heartbeat(&stop, Duration::from_millis(50));
        let elapsed = start.elapsed();
        assert_eq!(outcome, WaitOutcome::Heartbeat);
        assert!(
            elapsed >= Duration::from_millis(50),
            "should have waited the full deadline"
        );
    }

    #[test]
    fn wait_returns_notified_on_generation_bump() {
        let watcher = PowerWatcher::from_initial_state(false);
        let inner = watcher.inner.clone();
        let stop = AtomicBool::new(false);
        // Fire a notify on a sibling thread so the wait returns early.
        let inner_clone = inner.clone();
        let notifier = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            {
                let mut guard = inner_clone.predicate_lock.lock();
                guard.notify_count = guard.notify_count.wrapping_add(1);
                inner_clone
                    .notify_count
                    .store(guard.notify_count, Ordering::Release);
            }
            inner_clone.condvar.notify_all();
        });
        let start = Instant::now();
        let outcome = watcher.wait_for_heartbeat(&stop, HEARTBEAT);
        let elapsed = start.elapsed();
        notifier.join().unwrap();
        assert_eq!(outcome, WaitOutcome::Notified);
        assert!(
            elapsed < HEARTBEAT,
            "should have returned on notify, not heartbeat"
        );
    }
}
