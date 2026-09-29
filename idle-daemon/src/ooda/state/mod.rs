// SPDX-License-Identifier: MIT

//! openOODA Pillar 5: State (Centralized State Store & D-Bus Contract Publisher)

use crate::controller::DaemonController;

#[derive(Default)]
pub struct OodaStateManager;

impl OodaStateManager {
    pub fn new() -> Self {
        Self
    }

    /// Synchronize internal live state with canonical `DaemonStatus` and publish status if dirty.
    #[allow(clippy::fn_params_excessive_bools)]
    pub fn sync_state(
        &self,
        controller: &DaemonController,
        system_idle: bool,
        presentation_active: bool,
        preview_active: bool,
        current_saver: &str,
        effective_inhibited: bool,
    ) {
        controller.update_live_state(
            system_idle,
            presentation_active,
            preview_active,
            current_saver,
            effective_inhibited,
        );
        controller.publish_status_if_dirty();
    }
}
