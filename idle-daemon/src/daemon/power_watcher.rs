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
//! only sane behavior when the source dir is missing. Logged once at
//! startup, not per frame.
//!
//! Design notes:
//!
//!   * `cached_is_on_battery()` reads from an atomic — no Mutex on the
//!     hot path. The Mutex only guards the notify state (last_event_count)
//!     and is dropped before the condvar wake. `parking_lot::Mutex` over
//!     `std::sync::Mutex` to keep the lock-release path branch-free on
//!     the daemon hot path.
//!   * `wait_for_heartbeat()` is the consumer side: predicate-based wait
//!     that returns `WaitOutcome::Notified` on a real event,
//!     `WaitOutcome::Heartbeat` on the 1-second timeout, and never blocks
//!     past the supplied stop-flag flip — predicate re-checks
//!     `stop.load(Relaxed)` on every wake.
//!   * Cache invalidation uses a generation counter so spurious notifies
//!     from udev-induced churn don't produce redundant OODA re-reads.

use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use super::battery;

/// 1-second heartbeat: long enough to drive the watchdog on machines with
/// no power events, short enough to keep OODA responsive at idle.
pub const HEARTBEAT: Duration = Duration::from_secs(1);

/// inotify event mask covering what `/sys/class/power_supply` cares about.
/// `IN_MODIFY` (status files write through), `IN_CREATE` / `IN_DELETE` /
/// `IN_MOVED_*` (USB-C / dock hotplug), `IN_ATTRIB` (chmod/owner), plus
/// overflow catching if the kernel queue ever saturates.
const WATCH_MASK: u32 = libc::IN_MODIFY
    | libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_CLOSE_WRITE
    | libc::IN_ATTRIB
    | libc::IN_Q_OVERFLOW;

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
    inner: Arc<Inner>,
}

struct Inner {
    /// Atomic snapshot of `is_on_battery()`. Updated by the watcher thread;
    /// read by every OODA tick without taking the lock.
    cached_on_battery: AtomicBool,
    /// Generation counter — increments on every notify. Lets the consumer
    /// distinguish "wakeup was a real event" from "spurious condvar wake".
    notify_count: AtomicU64,
    /// Mutex guarding the condvar predicate. parking_lot so we don't
    /// allocate Guard objects or take the slow path on a clean release.
    predicate_lock: Mutex<Predicate>,
    condvar: Condvar,
}

struct Predicate {
    notify_count: u64,
}

/// Watcher handle + a guard thread. Drop the watcher to stop the background
/// thread (closes the inotify fd and joins).
pub struct PowerWatcherThread {
    handle: PowerWatcher,
    fd: libc::c_int,
    stop_flag: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PowerWatcher {
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

impl PowerWatcherThread {
    /// Spawn the inotify watcher. Returns `Ok(handle)` if inotify was
    /// usable, or `Err(_)` if the syscall path failed.
    ///
    /// On error, callers should fall back to the prior polling path. The
    /// `battery::is_on_battery()` polling is still correct (just costs
    /// more idle CPU); we don't want to mask a real bug behind a silent
    /// watcher fallback.
    pub fn spawn() -> io::Result<Self> {
        let dir = Path::new("/sys/class/power_supply");
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let c_dir = std::ffi::CString::new(dir.as_os_str().as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let wd = unsafe { libc::inotify_add_watch(fd, c_dir.as_ptr(), WATCH_MASK) };
        if wd < 0 {
            let err = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(err);
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let inner = Arc::new(Inner {
            cached_on_battery: AtomicBool::new(battery::is_on_battery()),
            notify_count: AtomicU64::new(0),
            predicate_lock: Mutex::new(Predicate { notify_count: 0 }),
            condvar: Condvar::new(),
        });
        // First sample before the thread starts so the daemon's first
        // OODA tick reads the correct value without a wasted cycle.
        let _ = inner.cached_on_battery.load(Ordering::Relaxed);

        let thread_inner = inner.clone();
        let thread_stop = stop_flag.clone();
        let thread = std::thread::Builder::new()
            .name("idle-power-watch".into())
            .spawn(move || run_loop(fd, thread_inner, thread_stop))?;

        Ok(Self {
            handle: PowerWatcher { inner },
            fd,
            stop_flag,
            thread: Some(thread),
        })
    }

    /// Consumer-side cloneable handle.
    pub fn handle(&self) -> PowerWatcher {
        self.handle.clone()
    }
}

impl Drop for PowerWatcherThread {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        // Closing the inotify fd wakes the poll loop, so the thread exits
        // even if no event ever fires.
        unsafe { libc::close(self.fd) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run_loop(fd: libc::c_int, inner: Arc<Inner>, stop: Arc<AtomicBool>) {
    let mut buf = [0u8; 8192];
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let mut fds = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // 250 ms cap so a stop signal flips the loop without a self-pipe.
        let ready = unsafe { libc::poll(&mut fds, 1, 250) };
        if ready <= 0 {
            continue;
        }
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted || e.kind() == io::ErrorKind::WouldBlock {
                    continue;
                }
            }
            return;
        }
        // Recompute the cached battery state. Even one event in the buffer
        // is enough to invalidate; we don't track per-file deltas.
        let _ = consume_events(&buf[..n as usize]);
        let on_battery = battery::is_on_battery();
        inner.cached_on_battery.store(on_battery, Ordering::Relaxed);
        // Bump generation + notify. Lock acquisition is uncontended; the
        // condvar wake is amortised across all events in the buffer.
        {
            let mut guard = inner.predicate_lock.lock();
            guard.notify_count = guard.notify_count.wrapping_add(1);
            inner
                .notify_count
                .store(guard.notify_count, Ordering::Release);
        }
        inner.condvar.notify_all();
    }
}

/// Parse an inotify read buffer. Currently only consumes whole records —
/// we don't care *which* file changed, only *that something* did. Returns
/// the number of events seen so the caller can decide whether to refresh
/// the cache.
fn consume_events(buf: &[u8]) -> usize {
    let mut off = 0usize;
    let mut n = 0usize;
    while off + 16 <= buf.len() {
        let len = u32::from_ne_bytes([buf[off + 12], buf[off + 13], buf[off + 14], buf[off + 15]])
            as usize;
        let name_end = off + 16 + len;
        if name_end > buf.len() {
            break;
        }
        n += 1;
        let _ = &buf[off..off + 16]; // header read
        off = name_end;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_returns_stopped_when_stop_already_set() {
        let inner = Arc::new(Inner {
            cached_on_battery: AtomicBool::new(false),
            notify_count: AtomicU64::new(0),
            predicate_lock: Mutex::new(Predicate { notify_count: 0 }),
            condvar: Condvar::new(),
        });
        let watcher = PowerWatcher { inner };
        let stop = AtomicBool::new(true);
        let outcome = watcher.wait_for_heartbeat(&stop, HEARTBEAT);
        assert_eq!(outcome, WaitOutcome::Stopped);
    }

    #[test]
    fn wait_returns_heartbeat_on_deadline() {
        let inner = Arc::new(Inner {
            cached_on_battery: AtomicBool::new(false),
            notify_count: AtomicU64::new(0),
            predicate_lock: Mutex::new(Predicate { notify_count: 0 }),
            condvar: Condvar::new(),
        });
        let watcher = PowerWatcher { inner };
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
        let inner = Arc::new(Inner {
            cached_on_battery: AtomicBool::new(false),
            notify_count: AtomicU64::new(0),
            predicate_lock: Mutex::new(Predicate { notify_count: 0 }),
            condvar: Condvar::new(),
        });
        let watcher = PowerWatcher {
            inner: inner.clone(),
        };
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
