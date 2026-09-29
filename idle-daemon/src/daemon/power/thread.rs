// SPDX-License-Identifier: MIT

//! Background inotify thread that drives the [`PowerWatcher`].
//!
//! Owns the inotify fd, the stop flag, and the worker thread. Drop
//! closes the fd (which wakes the poll loop) and joins the worker
//! within a bounded wait.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::Mutex;

use super::battery;
use crate::daemon::consume_events::consume_events;
use super::watcher::{Inner, PowerWatcher, Predicate};

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

/// Watcher handle + a guard thread. Drop the watcher to stop the background
/// thread (closes the inotify fd and joins).
pub struct PowerWatcherThread {
    handle: PowerWatcher,
    fd: libc::c_int,
    stop_flag: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PowerWatcherThread {
    /// Spawn the inotify watcher. Returns `Ok(handle)` if inotify was
    /// usable, or `Err(_)` if the syscall path failed.
    ///
    /// On error, callers should fall back to the prior polling path.
    /// The `battery::is_on_battery()` polling is still correct (just
    /// costs more idle CPU); we don't want to mask a real bug behind
    /// a silent watcher fallback.
    pub fn spawn() -> io::Result<Self> {
        let dir = Path::new("/sys/class/power_supply");
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let c_dir = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes())
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
            condvar: parking_lot::Condvar::new(),
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

/// The worker loop: poll(2) for events, read the inotify buffer,
/// recompute the cached battery state, then bump the notify
/// generation + wake the condvar.
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

#[cfg(test)]
mod tests {
    use super::{Inner, Predicate};
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    #[test]
    fn inner_construction_round_trip() {
        // The Inner struct must be constructible from the fields
        // PowerWatcherThread::spawn uses, without unsafe.
        let inner = Inner {
            cached_on_battery: AtomicBool::new(false),
            notify_count: AtomicU64::new(0),
            predicate_lock: Mutex::new(Predicate { notify_count: 0 }),
            condvar: parking_lot::Condvar::new(),
        };
        assert!(!inner.cached_on_battery.load(Ordering::Relaxed));
        assert_eq!(inner.notify_count.load(Ordering::Acquire), 0);
    }
}
