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
                Some(_) => {
                    idle_log::info!("logind Lock signal received — activating screensaver");
                    let _ = controller.send_command(DaemonCommand::Activate);
                }
                None => break,
            },
            opt = next(&mut unlock_stream) => match opt {
                Some(_) => {
                    idle_log::info!("logind Unlock signal received — clearing session lock");
                    controller.session_locked.store(false, Ordering::Relaxed);
                }
                None => break,
            },
            opt = next(&mut hint_stream) => match opt {
                Some(change) => match change.get().await {
                    Ok(locked) => {
                        idle_log::info!("logind LockedHint changed: {locked}");
                        controller.session_locked.store(locked, Ordering::Relaxed);
                    }
                    Err(error) => idle_log::error!("LockedHint update failed: {error}"),
                },
                None => break,
            },
        }
    }
}
