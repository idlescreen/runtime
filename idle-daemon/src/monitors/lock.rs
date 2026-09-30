// SPDX-License-Identifier: MIT

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::controller::{DaemonCommand, DaemonController};
use crate::futures_util::next;

#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1/session/auto"
)]
trait LogindSession {
    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;

    #[zbus(signal)]
    fn lock(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn unlock(&self) -> zbus::Result<()>;
}

pub async fn watch_session_lock(controller: Arc<DaemonController>) {
    let connection = match zbus::Connection::system().await {
        Ok(connection) => connection,
        Err(error) => {
            idle_log::error!("logind lock monitor unavailable: {error}");
            return;
        }
    };

    let proxy = match LogindSessionProxy::new(&connection).await {
        Ok(proxy) => proxy,
        Err(error) => {
            idle_log::error!("logind session proxy unavailable: {error}");
            return;
        }
    };

    match proxy.locked_hint().await {
        Ok(locked) => controller.session_locked.store(locked, Ordering::Relaxed),
        Err(error) => idle_log::error!("failed to read LockedHint: {error}"),
    }

    let mut lock_stream = match proxy.receive_lock().await {
        Ok(s) => s,
        Err(e) => {
            idle_log::error!("failed to subscribe to Lock signal: {e}");
            return;
        }
    };

    let mut unlock_stream = match proxy.receive_unlock().await {
        Ok(s) => s,
        Err(e) => {
            idle_log::error!("failed to subscribe to Unlock signal: {e}");
            return;
        }
    };

    let mut hint_stream = proxy.receive_locked_hint_changed().await;

    while !controller.shutdown.load(Ordering::Relaxed) {
        tokio::select! {
            opt = next(&mut lock_stream) => match opt {
                Some(_) => handle_lock_signal(&controller),
                None => break,
            },
            opt = next(&mut unlock_stream) => match opt {
                Some(_) => handle_unlock_signal(&controller),
                None => break,
            },
            opt = next(&mut hint_stream) => match opt {
                Some(change) => match change.get().await {
                    Ok(locked) => handle_locked_hint_change(&controller, locked),
                    Err(error) => idle_log::error!("LockedHint update failed: {error}"),
                },
                None => break,
            },
        }
    }
}

pub(crate) fn handle_lock_signal(controller: &DaemonController) {
    idle_log::info!("logind Lock signal received — activating screensaver");
    let _ = controller.send_command(DaemonCommand::Activate);
}

pub(crate) fn handle_unlock_signal(controller: &DaemonController) {
    idle_log::info!("logind Unlock signal received — clearing session lock");
    controller.session_locked.store(false, Ordering::Relaxed);
    controller.mark_dirty();
}

pub(crate) fn handle_locked_hint_change(controller: &DaemonController, locked: bool) {
    idle_log::info!("logind LockedHint changed: {locked}");
    controller.session_locked.store(locked, Ordering::Relaxed);
    controller.mark_dirty();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DaemonConfig;

    #[test]
    fn test_handle_lock_signal_dispatches_activate() {
        let controller = DaemonController::new(DaemonConfig::default());
        handle_lock_signal(&controller);
        let commands = controller.drain_commands();
        assert!(commands.contains(&DaemonCommand::Activate));
    }

    #[test]
    fn test_handle_unlock_signal_clears_lock() {
        let controller = DaemonController::new(DaemonConfig::default());
        controller.session_locked.store(true, Ordering::Relaxed);
        handle_unlock_signal(&controller);
        assert!(!controller.session_locked.load(Ordering::Relaxed));
        assert!(controller.take_dirty());
    }

    #[test]
    fn test_handle_locked_hint_change() {
        let controller = DaemonController::new(DaemonConfig::default());
        handle_locked_hint_change(&controller, true);
        assert!(controller.session_locked.load(Ordering::Relaxed));
        assert!(controller.take_dirty());

        handle_locked_hint_change(&controller, false);
        assert!(!controller.session_locked.load(Ordering::Relaxed));
        assert!(controller.take_dirty());
    }
}
