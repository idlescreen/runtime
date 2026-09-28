// SPDX-License-Identifier: MIT
// perf: T3 · metric: bounded single-pass work; no syscalls, no locks, no allocation on the steady path · check: review

//! OOP plugin process liveness and crash recovery.
//!
//! **Single reaper rule:** only this session reaps the runner pid via
//! `Child::try_wait` / `Child::wait` (never a side-thread `waitpid`).
//! `Child` Drop does **not** wait — callers must `kill`+`wait` on teardown
//! and on init failure (see `ipc_init::kill_and_reap`).

use super::ipc_session::IpcPluginSession;
use idle_ipc::IpcCommand;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

impl IpcPluginSession {
    /// True if the OOP plugin child is still running.
    ///
    /// An unexpected exit is logged and nothing more. IdleScreen is a
    /// screensaver, not a lock: per `DESIGN.md` it must never call
    /// `loginctl` or `swaylock` to lock the user's session, so there is
    /// deliberately no locker here. Frame loop latency is one tick.
    pub fn is_plugin_alive(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                if !self.expected_stop.load(Ordering::Relaxed) {
                    idle_log::error!(?status, "plugin child exited unexpectedly");
                }
                false
            }
            Err(e) => {
                // ECHILD should not occur under the single-reaper rule; treat as dead.
                idle_log::error!(%e, "plugin child status query failed");
                false
            }
        }
    }

    /// Default number of consecutive IPC timeouts tolerated before the
    /// session gives up instead of respawning the child again.
    pub const DEFAULT_MAX_RUNNER_TIMEOUTS: u32 = 3;

    /// Consecutive-timeout budget for one child, overridable by operators for
    /// genuinely slow savers on weak hardware.
    pub fn max_consecutive_timeouts() -> u32 {
        std::env::var("IDLE_MAX_RUNNER_TIMEOUTS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(Self::DEFAULT_MAX_RUNNER_TIMEOUTS)
    }

    /// Whether this session may respawn its child at `cols`×`rows`.
    pub fn should_recover(&self, cols: usize, rows: usize) -> bool {
        self.consecutive_timeouts < Self::max_consecutive_timeouts()
            && self.exhausted_geometry != Some((cols, rows))
    }

    /// Record a successful frame — clears the timeout run.
    pub(crate) fn note_progress(&mut self) {
        self.consecutive_timeouts = 0;
    }

    /// Record an IPC timeout against the live child.
    pub(crate) fn note_timeout(&mut self) {
        self.consecutive_timeouts = self.consecutive_timeouts.saturating_add(1);
    }

    /// Called once the budget is spent: remember the geometry so a later
    /// presentation at the same size fails fast instead of respawn-looping.
    pub(crate) fn mark_exhausted(&mut self, cols: usize, rows: usize) {
        self.exhausted_geometry = Some((cols, rows));
    }

    /// Tear down and re-spawn the OOP plugin process (crash isolation).
    pub fn recover(&mut self, cols: usize, rows: usize) -> Result<(), String> {
        idle_log::warn!(saver = %self.saver_name, "recovering OOP plugin session");
        self.expected_stop.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.socket = None;
        if let Some(path) = self.socket_path.take() {
            let _ = fs::remove_file(path);
        }
        self.shm = None;
        self.expected_stop = Arc::new(AtomicBool::new(false));
        self.current_geometry = Some((cols, rows));
        self.init(cols, rows)
    }
}

impl Drop for IpcPluginSession {
    fn drop(&mut self) {
        self.expected_stop.store(true, Ordering::Relaxed);
        if let Some(ref mut socket) = self.socket {
            let _ = IpcCommand::Stop.write_to(&mut *socket);
        }
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(ref socket_path) = self.socket_path {
            let _ = fs::remove_file(socket_path);
        }
    }
}
