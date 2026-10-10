// SPDX-License-Identifier: MIT

#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![warn(clippy::panic)]
#![warn(clippy::todo)]
#![warn(clippy::unimplemented)]

pub mod config;
pub mod futures_util;

pub mod affinity;
pub mod cli_main;
pub mod controller;
pub mod daemon;
pub mod dbus_server;
pub mod inhibit;
pub mod ipc_runner;
pub mod monitors;
pub use monitors::locks;
pub mod ooda;
pub mod presentation;
pub use daemon::notify;

// Re-exports for `benches/draw_frame.rs`.
//
// `apply_fade_in` is the T1 page (gated); `consume_events` and
// `power_watcher` are T2 (benched on demand, not gated). A `[[bench]]`
// target compiles as its own crate, so each one's owning module
// re-exports them through a `#[doc(hidden)]` seam before they land
// here. Mirrors `savers/ripple/src/lib.rs::bench_exports`.
#[doc(hidden)]
pub mod bench_exports {
    pub use crate::daemon::bench_exports::*;
    pub use crate::presentation::bench_exports::*;
}

/// Shared mutex for tests that mutate process env. Environment is
/// process-global and the whole lib test suite runs in one process —
/// file-local locks can't exclude siblings in other modules. Any test
/// that sets/removes an env var must hold this for its full body.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
