// SPDX-License-Identifier: Apache-2.0
// perf: T3 · metric: lock-sensitive; cost depends on contention the caller creates · check: test
// Copyright 2026 IdleScreen

//! Frame-presented signal — used by the daemon's frame loop to wake
//! from vsync-relative sleeps without polling.
//!
//! Two notification paths feed `FrameSignal::notify()` today:
//!
//!   * **Commit path** (post-`update_frame`): the previous frame is
//!     queued for presentation. This is the conservative path — it
//!     drops the 2 ms slice-poll that was waking the daemon ~30×/sec
//!     even when no frames presented.
//!   * **`wl_callback::done` path** (deferred): would notify on actual
//!     vsync once the compositor presents the frame. The dispatch
//!     hook in `handlers/buffer_objects.rs` is currently empty; when
//!     we wire it up, it just calls `frame_signal.notify()` and the
//!     frame loop wakes at true vsync. Filed as a follow-up commit
//!     after this one.
//!
//! Wake-source contract: the producer (`notify`) bumps a generation
//! counter under a parking_lot Mutex, then calls `notify_all`. The
//! consumer (`wait_for`) records the generation when entering the
//! wait, then in `wait_while_for`'s closure predicate checks both
//! "generation moved" and "stop flipped".
//!
//! Why `parking_lot::Condvar` over `std::sync::Condvar`:
//!
//!   * Drop-the-guard semantic — `Condvar::wait_*` takes
//!     `&mut MutexGuard`, so the locked region stays tight.
//!   * Closure-predicate form is `wait_while_for(...)` rather than
//!     the manual retry-loop required for `std::sync`.
//!   * Existing transitive `parking_lot = "0.12"` in the runtime
//!     workspace makes this a zero-new-dep addition.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

/// Outcome of [`FrameSignal::wait_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameWaitOutcome {
    /// A frame was committed (or vsync happened, once wired). Consumer
    /// should re-check whether it has work to schedule; if not, wait
    /// again with the remaining frame budget.
    Notified,
    /// Wait deadline elapsed with no frame-arrival. Usually means a
    /// frame ran over its budget; consumer should drop the frame
    /// rather than try to catch up.
    TimedOut,
    /// Stop flag flipped. Caller should exit.
    Stopped,
}

/// Shared, cloneable handle. Cheap (Arc bump).
#[derive(Clone)]
pub struct FrameSignal {
    inner: Arc<Inner>,
}

struct Inner {
    notify_count: AtomicU64,
    predicate_lock: Mutex<Predicate>,
    condvar: Condvar,
}

struct Predicate {
    notify_count: u64,
}

impl FrameSignal {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                notify_count: AtomicU64::new(0),
                predicate_lock: Mutex::new(Predicate { notify_count: 0 }),
                condvar: Condvar::new(),
            }),
        }
    }

    /// Bump generation + wake one waiter. O(1).
    pub fn notify(&self) {
        let mut guard = self.inner.predicate_lock.lock();
        guard.notify_count = guard.notify_count.wrapping_add(1);
        self.inner
            .notify_count
            .store(guard.notify_count, Ordering::Release);
        // Drop guard before notify_all — parking_lot::Condvar lets the
        // producer hold the lock through notify (it's wait_*, not
        // notify_*, that requires the lock release), but the
        // waiter-side predicate re-check uses the lock too. Holding
        // it longer than necessary just tightens the wakeup window.
        drop(guard);
        self.inner.condvar.notify_all();
    }

    /// Last observed generation. Useful for "did anything happen
    /// since I last checked?" without entering the wait.
    pub fn notify_generation(&self) -> u64 {
        self.inner.notify_count.load(Ordering::Acquire)
    }

    /// Block up to `max`. Returns the wake-class. Predicate-driven,
    /// so spurious wakes (very rare with parking_lot, but real) do
    /// not cause a wasted iteration of the frame loop.
    pub fn wait_for(&self, stop: &AtomicBool, max: Duration) -> FrameWaitOutcome {
        // Fast path: shutdown before we entered.
        if stop.load(Ordering::Relaxed) {
            return FrameWaitOutcome::Stopped;
        }
        let deadline = Instant::now() + max;
        let observed_generation = self.inner.notify_count.load(Ordering::Acquire);
        let mut guard = self.inner.predicate_lock.lock();
        loop {
            if stop.load(Ordering::Relaxed) {
                return FrameWaitOutcome::Stopped;
            }
            let now = Instant::now();
            if now >= deadline {
                return FrameWaitOutcome::TimedOut;
            }
            if guard.notify_count != observed_generation {
                return FrameWaitOutcome::Notified;
            }
            let remaining = deadline.saturating_duration_since(now);
            // wait_while_for: closure keeps waiting while predicate is
            // true (i.e., generation hasn't moved AND stop hasn't flipped).
            self.inner.condvar.wait_while_for(
                &mut guard,
                |p| !stop.load(Ordering::Relaxed) && p.notify_count == observed_generation,
                remaining,
            );
        }
    }
}

impl Default for FrameSignal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_stopped_when_stop_already_set() {
        let sig = FrameSignal::new();
        let stop = AtomicBool::new(true);
        let outcome = sig.wait_for(&stop, Duration::from_millis(50));
        assert_eq!(outcome, FrameWaitOutcome::Stopped);
    }

    #[test]
    fn returns_timed_out_on_deadline() {
        let sig = FrameSignal::new();
        let stop = AtomicBool::new(false);
        let start = Instant::now();
        let outcome = sig.wait_for(&stop, Duration::from_millis(50));
        let elapsed = start.elapsed();
        assert_eq!(outcome, FrameWaitOutcome::TimedOut);
        assert!(
            elapsed >= Duration::from_millis(50),
            "should have waited the full deadline"
        );
    }

    #[test]
    fn returns_notified_on_generation_bump() {
        let sig = FrameSignal::new();
        let stop = AtomicBool::new(false);
        let sig_clone = sig.clone();
        let notifier = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            sig_clone.notify();
        });
        let start = Instant::now();
        let outcome = sig.wait_for(&stop, Duration::from_millis(500));
        let elapsed = start.elapsed();
        notifier.join().unwrap();
        assert_eq!(outcome, FrameWaitOutcome::Notified);
        assert!(
            elapsed < Duration::from_millis(500),
            "should have returned on notify, not the full timeout"
        );
    }
}
