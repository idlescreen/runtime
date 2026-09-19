// SPDX-License-Identifier: MIT

use crate::controller::{DaemonCommand, DaemonController};
use idle_dbus::DaemonStatus;
use std::sync::Arc;

pub async fn authorize_control(
    controller: &Arc<DaemonController>,
    header: &zbus::message::Header<'_>,
) -> zbus::fdo::Result<()> {
    super::auth::require_control_peer(
        &controller
            .dbus_connection()
            .map_err(zbus::fdo::Error::Failed)?,
        header,
    )
    .await
}

/// Apply a config-mutating command, log failures, and sync status.
pub fn apply_config_command(
    controller: &Arc<DaemonController>,
    command: DaemonCommand,
    label: &str,
) -> zbus::fdo::Result<()> {
    if let Err(error) = controller.apply_command(command) {
        idle_log::error!(target: "idle_daemon::dbus", "{label} failed: {error:?}");
        return Err(zbus::fdo::Error::Failed(error.to_string()));
    }
    sync_config_status(controller);
    Ok(())
}

pub fn live_status(controller: &Arc<DaemonController>) -> DaemonStatus {
    let mut status = controller
        .status
        .lock()
        .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
        .clone();
    status.session_locked = controller
        .session_locked
        .load(std::sync::atomic::Ordering::Relaxed);
    let on_battery = crate::daemon::battery::is_on_battery();
    status.inhibited = status.inhibited || controller.inhibitors.is_inhibited() || on_battery;
    status
}

pub fn sync_config_status(controller: &Arc<DaemonController>) {
    let config = controller
        .config
        .lock()
        .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
        .clone();
    {
        let mut status = controller
            .status
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        status.idle_enabled = config.idle_enabled;
        status.idle_timeout_mins = config.idle_timeout_mins;
        status.active_saver = config.active_saver.clone().unwrap_or_default();
        status.show_fps_overlay = config.show_fps_overlay;
        status.render_scale = config
            .render_scale
            .map(|s| s.to_string())
            .unwrap_or_default();
    }
    controller.mark_dirty();
    controller.publish_status_if_dirty();
}
