// SPDX-License-Identifier: MIT
// perf: T3 · metric: bounded single-pass work; no syscalls, no locks, no allocation on the steady path · check: review

//! Wayland client thread for [`ext-idle-notify-v1`](https://wayland.app/protocols/ext-idle-notify-v1).
//!
//! Binds the idle notifier on a background thread and forwards state changes back
//! to the public [`IdleMonitor`](crate::IdleMonitor) handle. Protocol dispatch lives
//! in [`handlers`]; connection lifecycle is owned by [`thread`].
//!
//! The thread blocks in `dispatch` until the monitor is dropped or the compositor
//! disconnects. A failure to start — no Wayland connection, no
//! `ext-idle-notify-v1` global, no seat — is reported over the startup handshake
//! so `IdleMonitor::new` returns `None`; it is never silently degraded to a
//! monitor that reports "not idle" forever. A failure *after* a successful
//! start clears `is_alive` and the caller observes it through
//! `IdleMonitor::is_alive`.
//!
//! # Safety
//!
//! All Wayland objects are created and destroyed on this thread; do not call
//! `spawn_event_thread` more than once per [`IdleMonitor`](crate::IdleMonitor).

mod handlers;
mod state;
mod thread;

pub use thread::spawn_event_thread;

// Idle notifications are coalesced before crossing the thread boundary.
// Registry globals are bound once per monitor instance.
// Disconnecting the queue ends the monitor thread cleanly.
// Handlers are split to keep each Dispatch impl file under 256 lines.
