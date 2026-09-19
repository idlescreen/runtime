// SPDX-License-Identifier: MIT

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
use std::sync::atomic::{AtomicBool, Ordering};

/// Atomically take the failsafe arm. Returns true only on the first successful take
/// until re-armed (e.g. after `recover`). Pure once-gate for unit tests.
pub(crate) fn take_failsafe_arm(armed: &AtomicBool) -> bool {
    armed
        .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

impl IpcPluginSession {
    /// True if the OOP plugin child is still running.
    ///
    /// On unexpected exit, triggers the failsafe locker once (session lock if
    /// the saver process died while presenting). Frame loop latency is one tick.
    pub fn is_plugin_alive(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                if !self.expected_stop.load(Ordering::Relaxed) {
                    idle_log::error!(?status, "plugin child exited unexpectedly");
                    self.trigger_failsafe_once();
                }
                false
            }
            Err(e) => {
                // ECHILD should not occur under the single-reaper rule; treat as dead.
                idle_log::error!(%e, "plugin child status query failed");
                if !self.expected_stop.load(Ordering::Relaxed) {
                    self.trigger_failsafe_once();
                }
                false
            }
        }
    }

    fn trigger_failsafe_once(&self) {
        // `expected_stop` is the intentional-stop flag; failsafe_armed gates once.
        if take_failsafe_arm(&self.failsafe_armed)
            && let Err(e) = crate::failsafe::spawn_failsafe_locker()
        {
            idle_log::error!("failsafe: failed to spawn locker after runner death: {e}");
        }
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
        self.failsafe_armed = Arc::new(AtomicBool::new(true));
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

#[cfg(test)]
#[path = "ipc_lifecycle_tests.rs"]
mod tests;
