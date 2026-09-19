// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

//! Vendored subset of the `library` crate, scoped to what screensaver-security
//! actually uses. See `LIBRARY_VENDORED.md` for the full list of source
//! provenance and what was deliberately omitted.
//!
//! Original (pre-vendoring) location: /home/jeryd/library (crateria/library).
//! Vendored on: 2026-06-12 from tag v2026.6.10.
//! License: MIT (see top-level LICENSE file).
//!
//! ## Modules
//!
//! - [`cell_renderer`] — terminal grid → BGRA framebuffer rasterization
//! - [`core`] — shared primitives (logo block, palette, screensaver traits)
//! - [`discovery`] — locate installed screensaver plugins on disk
//! - [`launcher`] — validate and spawn plugin binaries
//! - [`plugin_session`] — load, tick, and render a plugin for presentation
//! - [`toolkit`] — host queries (system info, theme, platform metadata)
//! - [`idle_runner`] — fullscreen plugin runner for manual testing

pub mod apps;
pub mod budget;
pub mod caption_overlay;
pub mod cell_renderer;
pub mod core;
pub mod discovery;
pub mod dylib;
pub mod filewatch;
pub mod fps_overlay;
pub mod gpu_budget;
pub mod idle_runner;
pub mod launcher;
mod launcher_resolve;
mod launcher_trust;
pub mod plugin_session;
pub mod sandbox;
pub mod sandbox_profiles;
pub mod toolkit;
pub mod watchdog;

// Tests can run with `cargo test -- --nocapture` to see tracing output.

/// Serializes tests that mutate process env vars (e.g.
/// `IDLE_ALLOW_UNSIGNED_PLUGINS`). Per-module locks do NOT serialize —
/// every env-touching test must take this one shared lock.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) mod test_util {
    /// `tempfile::tempdir` replacement: unique dir under the system temp
    /// root, removed on drop.
    pub(crate) struct TmpDir(std::path::PathBuf);

    impl TmpDir {
        pub(crate) fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub(crate) fn tempdir() -> std::io::Result<TmpDir> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        for _ in 0..100 {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = base.join(format!("idle-runner-test-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(TmpDir(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate unique temp dir",
        ))
    }
}

#[cfg(test)]
#[path = "abi_version_tests.rs"]
mod abi_version_tests;

#[cfg(test)]
#[path = "frame_perf_tests.rs"]
mod frame_perf_tests;

#[cfg(test)]
#[path = "plugin_manifest_tests.rs"]
mod plugin_manifest_tests;

#[cfg(test)]
#[path = "capability_gate_tests.rs"]
mod capability_gate_tests;

#[cfg(test)]
#[path = "c_abi_conformance_tests.rs"]
mod c_abi_conformance_tests;
