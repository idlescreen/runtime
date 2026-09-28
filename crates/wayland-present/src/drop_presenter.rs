// SPDX-License-Identifier: MIT

// perf: T2 · bench: hot_path · on-demand only; not gated
//! Bounded shutdown of the overlay presenter.
//!
//! `Drop for OverlayPresenter` is the one place where the daemon
//! says "we're done" to the event thread. The teardown order is
//! load-bearing: shutdown flag first, wake second, command-channel
//! signal third, bounded join last. A wedged event thread must not
//! hang daemon shutdown forever; leaking the joiner is the lesser
//! evil.

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::overlay::command::PresenterCommand;

use super::OverlayPresenter;

impl Drop for OverlayPresenter {
    fn drop(&mut self) {
        // Set shutdown and wake FIRST. `command_tx` is a `sync_channel(1)`,
        // so a blocking `send` here would hang Drop itself when the channel
        // is full — and a wedged event thread (compositor not draining the
        // socket) would pin shutdown forever, which is exactly what the
        // bounded join below exists to prevent. A `try_send` that loses the
        // race to a full channel is fine: `shutdown` is what the loop
        // actually polls.
        self.shutdown.store(true, Ordering::Relaxed);
        // Bare wake so the event loop sees `shutdown` promptly even if the
        // command channel is already drained.
        self.wake();

        let _ = self.command_tx.try_send(PresenterCommand::Hide);

        // Bounded join: the poll loop turns over in ≤100ms, so teardown
        // completes well under this bound on a healthy compositor. A
        // bounded wait beats an unbounded join — a wedged event thread
        // must not hang daemon shutdown forever; leaking the joiner is
        // the lesser evil.
        if let Some(handle) = self.event_thread.take() {
            join_with_timeout(handle, Duration::from_secs(2));
        }
    }
}

/// Bounded `JoinHandle::join` via a watcher thread + `recv_timeout`.
/// Keeps Drop from blocking indefinitely on a wedged event thread.
///
/// `pub` only so `bench_exports` below can re-export it to the
/// `hot_path` bench target. `mod drop_presenter` is private in
/// `lib.rs`, so this adds no reachable path on its own.
pub fn join_with_timeout(handle: JoinHandle<()>, timeout: Duration) {
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = handle.join();
        let _ = done_tx.send(());
    });
    if done_rx.recv_timeout(timeout).is_err() {
        idle_log::warn!("wayland-present: event thread did not exit within timeout");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_with_timeout_returns_quickly_for_fast_thread() {
        // A thread that exits immediately must release the watcher
        // well before the 2-second cap.
        let handle = std::thread::spawn(|| {
            // no work — exits as soon as Drop runs.
        });
        let start = std::time::Instant::now();
        join_with_timeout(handle, Duration::from_secs(2));
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "fast thread should release well before the timeout, got {elapsed:?}"
        );
    }

    #[test]
    fn join_with_timeout_unwinds_for_slow_thread() {
        // A thread that sleeps 5s with a 100ms cap — the watcher
        // must return after ~100ms, leaking the joiner.
        let handle = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(5));
        });
        let start = std::time::Instant::now();
        join_with_timeout(handle, Duration::from_millis(100));
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "bounded join must return at the timeout, got {elapsed:?}"
        );
    }
}

// Measurement seam, re-exported to `lib.rs::bench_exports` for the
// `[[bench]] hot_path` target. See RULES.md §5.
#[doc(hidden)]
pub mod bench_exports {
    pub use super::join_with_timeout;
}
