// SPDX-License-Identifier: MIT

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wayland_client::QueueHandle;
use wayland_client::protocol::wl_seat;
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1, ext_idle_notifier_v1,
};

/// Mutable Wayland session state owned by the background event thread.
pub struct SessionState {
    pub notifier: Option<ext_idle_notifier_v1::ExtIdleNotifierV1>,
    pub seat: Option<wl_seat::WlSeat>,
    pub notification: Option<ext_idle_notification_v1::ExtIdleNotificationV1>,
    pub is_idle: Arc<AtomicBool>,
    pub queue: QueueHandle<SessionState>,
    pub timeout_mins: u32,
}

impl SessionState {
    pub fn refresh_idle_notification(&mut self) {
        if let Some(notification) = self.notification.take() {
            notification.destroy();
        }

        self.is_idle.store(false, Ordering::SeqCst);

        let (Some(notifier), Some(seat)) = (&self.notifier, &self.seat) else {
            idle_log::warn!("wayland-idle: compositor missing seat or idle notifier global");
            return;
        };

        let timeout_ms = self.timeout_mins.saturating_mul(60).saturating_mul(1000);
        let notification = notifier.get_idle_notification(timeout_ms, seat, &self.queue, ());
        self.notification = Some(notification);

        idle_log::info!(
            "wayland-idle: registered idle notification (timeout {}s)",
            self.timeout_mins.saturating_mul(60)
        );
    }

    pub fn mark_idle(&self) {
        self.is_idle.store(true, Ordering::SeqCst);
        idle_log::info!("wayland-idle: system went idle");
    }

    pub fn mark_active(&self) {
        self.is_idle.store(false, Ordering::SeqCst);
        idle_log::info!("wayland-idle: user activity resumed");
    }
}
