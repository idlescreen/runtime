// SPDX-License-Identifier: MIT

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::wayland;

/// Tracks user inactivity through the Wayland `ext-idle-notify-v1` protocol.
///
/// Returns `None` from [`Self::new`] when `WAYLAND_DISPLAY` is unset **or** the
/// compositor does not expose the idle notifier. The second case is the one
/// that matters: a monitor constructed successfully that can never report idle
/// leaves a daemon that looks healthy and never blanks the screen.
pub struct IdleMonitor {
    is_idle: Arc<AtomicBool>,
    timeout_tx: mpsc::Sender<u32>,
    shutdown: Arc<AtomicBool>,
    is_alive: Arc<AtomicBool>,
    /// Joined in Drop so libwayland teardown finishes on the event thread
    /// before we unwind past it. A detached thread racing process exit is the
    /// teardown SIGSEGV class; `wayland_present::OverlayPresenter` carries
    /// the matching note.
    event_thread: Option<JoinHandle<()>>,
}

impl IdleMonitor {
    /// Connect to the current Wayland session and begin monitoring idle state.
    ///
    /// `timeout_mins` is the initial inactivity threshold. Use [`Self::new_timeout`]
    /// to pass a [`Duration`]; this constructor is preserved for callers that
    /// already pass minutes.
    pub fn new(timeout_mins: u32) -> Option<Self> {
        Self::new_timeout(Duration::from_secs(timeout_mins.saturating_mul(60) as u64))
    }

    /// Connect using a [`Duration`]. Internally the Wayland compositor's
    /// `ext-idle-notify-v1` only accepts a minute granularity, so the
    /// duration is rounded down to whole minutes.
    pub fn new_timeout(timeout: Duration) -> Option<Self> {
        if !Self::is_available() {
            return None;
        }

        let timeout_mins = (timeout.as_secs() / 60).min(u32::MAX as u64) as u32;

        let is_idle = Arc::new(AtomicBool::new(false));
        let (timeout_tx, timeout_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let is_alive = Arc::new(AtomicBool::new(true));

        let event_thread = wayland::spawn_event_thread(
            ready_tx,
            is_idle.clone(),
            shutdown.clone(),
            timeout_rx,
            timeout_mins,
            is_alive.clone(),
        );

        // Startup handshake. Without this the constructor cannot tell a healthy
        // monitor from one whose thread already gave up because the compositor
        // has no ext-idle-notify-v1 — the daemon would then run for the whole
        // session without ever blanking the screen.
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Some(Self {
                is_idle,
                timeout_tx,
                shutdown,
                is_alive,
                event_thread: Some(event_thread),
            }),
            other => {
                // Failed init (thread reported Err, or the ready channel timed
                // out / closed) — still join so the failed thread's teardown
                // does not outlive the constructor.
                shutdown.store(true, Ordering::Relaxed);
                let _ = event_thread.join();
                idle_log::warn!("wayland-idle: monitor init failed: {other:?}");
                None
            }
        }
    }

    /// Whether `WAYLAND_DISPLAY` is set in the environment.
    pub fn is_available() -> bool {
        std::env::var("WAYLAND_DISPLAY").is_ok()
    }

    /// Returns `true` when the user has been idle longer than the configured timeout.
    pub fn is_idle(&self) -> bool {
        self.is_idle.load(Ordering::SeqCst)
    }

    /// Returns `true` if the Wayland event monitoring thread is still running.
    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::SeqCst)
    }

    /// Update the idle timeout. The compositor is re-notified on the next event-loop tick.
    pub fn set_timeout(&self, timeout_mins: u32) {
        let _ = self.timeout_tx.send(timeout_mins);
    }

    /// Update the idle timeout from a [`Duration`]. Rounded down to whole minutes.
    pub fn set_timeout_duration(&self, timeout: Duration) {
        let mins = (timeout.as_secs() / 60).min(u32::MAX as u64) as u32;
        self.set_timeout(mins);
    }
}

impl Drop for IdleMonitor {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.event_thread.take() {
            let _ = handle.join();
        }
    }
}

impl idle_api::IdleSource for IdleMonitor {
    fn is_available() -> bool {
        IdleMonitor::is_available()
    }

    fn new(timeout: Duration) -> Option<Self> {
        IdleMonitor::new_timeout(timeout)
    }

    fn is_idle(&self) -> bool {
        IdleMonitor::is_idle(self)
    }

    fn is_alive(&self) -> bool {
        IdleMonitor::is_alive(self)
    }

    fn set_timeout(&self, timeout: Duration) {
        IdleMonitor::set_timeout_duration(self, timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_starts_unavailable_without_wayland() {
        let _lock = crate::get_test_mutex().lock().unwrap();
        let backup = std::env::var("WAYLAND_DISPLAY").ok();
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
        }
        assert!(!IdleMonitor::is_available());
        assert!(IdleMonitor::new(5).is_none());
        if let Some(val) = backup {
            unsafe {
                std::env::set_var("WAYLAND_DISPLAY", val);
            }
        }
    }

    #[test]
    fn monitor_is_available_matches_env() {
        let _lock = crate::get_test_mutex().lock().unwrap();
        let backup = std::env::var("WAYLAND_DISPLAY").ok();
        unsafe {
            std::env::set_var("WAYLAND_DISPLAY", "wayland-mock-monitor-0");
        }
        assert!(IdleMonitor::is_available());
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
        }
        assert!(!IdleMonitor::is_available());
        if let Some(val) = backup {
            unsafe {
                std::env::set_var("WAYLAND_DISPLAY", val);
            }
        }
    }

    /// The S1: with `WAYLAND_DISPLAY` set but no compositor behind it (the
    /// GNOME/Mutter case, which has no `ext-idle-notify-v1`), the constructor
    /// must report the monitor unavailable rather than hand back a handle whose
    /// thread is already dead and whose `is_idle` is pinned false forever.
    #[test]
    fn monitor_returns_none_when_compositor_is_unusable() {
        let _lock = crate::get_test_mutex().lock().unwrap();
        let backup = std::env::var("WAYLAND_DISPLAY").ok();
        unsafe {
            std::env::set_var("WAYLAND_DISPLAY", "wayland-mock-no-such-compositor");
        }
        let monitor = IdleMonitor::new_timeout(Duration::from_mins(5));
        assert!(
            monitor.is_none(),
            "a monitor that cannot bind ext-idle-notify-v1 must be None, not a live-but-dead handle"
        );
        if let Some(val) = backup {
            unsafe {
                std::env::set_var("WAYLAND_DISPLAY", val);
            }
        }
    }
}
