// SPDX-License-Identifier: MIT

//! Suspend/resume awareness via logind `PrepareForSleep`.
//!
//! The render-loop watchdog measures wall-clock heartbeat age — suspend
//! time counts. Without this watcher, any sleep longer than the watchdog
//! timeout (default 5s) reads as a stall on resume: the daemon raises
//! `shutdown`, exits non-zero, and systemd pays a full restart (D-Bus
//! re-export, IPC runner respawn, GPU probe) for a machine that merely
//! slept. On `start=false` we refresh the heartbeat baseline so resume
//! is treated as progress, not a stall.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::futures_util::next;

use crate::daemon::watchdog::Watchdog;

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait LogindManager {
    /// `PrepareForSleep(start)` — `true` entering sleep, `false` on resume.
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

pub async fn watch_prepare_for_sleep(watchdog: Watchdog, shutdown: Arc<AtomicBool>) {
    let connection = match zbus::Connection::system().await {
        Ok(connection) => connection,
        Err(error) => {
            idle_log::error!("logind sleep monitor unavailable: {error}");
            return;
        }
    };

    let proxy = match LogindManagerProxy::new(&connection).await {
        Ok(proxy) => proxy,
        Err(error) => {
            idle_log::error!("logind manager proxy unavailable: {error}");
            return;
        }
    };

    let mut stream = match proxy.receive_prepare_for_sleep().await {
        Ok(stream) => stream,
        Err(error) => {
            idle_log::error!("PrepareForSleep subscription failed: {error}");
            return;
        }
    };

    while !shutdown.load(Ordering::Relaxed) {
        match next(&mut stream).await {
            Some(signal) => match signal.args() {
                Ok(args) if args.start => {
                    idle_log::debug!("logind PrepareForSleep(true) — system suspending");
                }
                Ok(_) => {
                    watchdog.heartbeat();
                    idle_log::info!(
                        "logind PrepareForSleep(false) — resumed; watchdog baseline reset"
                    );
                }
                Err(error) => {
                    idle_log::warn!("malformed PrepareForSleep signal: {error}");
                }
            },
            None => break,
        }
    }
}
