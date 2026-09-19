// SPDX-License-Identifier: MIT

mod overlay;
mod overlay_frame;
mod overlay_geom;
mod types;

pub use types::{OutputTarget, SessionState};

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::appearance::OverlayAppearance;

impl SessionState {
    pub fn show_solid(&mut self, appearance: OverlayAppearance) {
        self.screensaver_mode = false;
        self.appearance = Some(appearance);
        self.begin_presentation();
    }

    pub fn show_screensaver(&mut self) {
        self.screensaver_mode = true;
        self.appearance = None;
        self.begin_presentation();
        idle_log::info!("wayland-present: screensaver surfaces ready for frames");
    }

    pub fn hide_pointer(&mut self, serial: u32) {
        if let Some(pointer) = &self.pointer {
            pointer.set_cursor(serial, None, 0, 0);
        }
    }

    fn begin_presentation(&mut self) {
        self.visible.store(true, Ordering::SeqCst);
        // 1-second grace window after the screensaver appears. Within this
        // window, pointer motion and key presses do NOT dismiss the
        // screensaver. This prevents the common case where the user
        // finishing a key press / mousemove just as the idle timer
        // expired would be instantly dismissed before they could
        // perceive the screensaver at all.
        //
        // The previous 800ms window was slightly too short: on a slow
        // monitor refresh or under load, the user could perceive the
        // overlay for less than a frame before it vanished.
        self.dismiss_grace_until = Some(Instant::now() + Duration::from_secs(1));
        self.output_registry.clear();
        if self.pointer_serial != 0 {
            self.hide_pointer(self.pointer_serial);
        }

        let output_ids: Vec<u32> = self.outputs.iter().map(|output| output.id).collect();
        for output_id in output_ids {
            self.create_overlay(output_id);
        }

        idle_log::info!(
            "wayland-present: showing overlay on {} output(s)",
            self.overlays.len()
        );
    }

    pub fn hide(&mut self) {
        self.appearance = None;
        self.screensaver_mode = false;
        self.visible.store(false, Ordering::SeqCst);
        self.output_registry.clear();

        for (_, overlay) in self.overlays.drain() {
            if let Some(viewport) = overlay.viewport {
                viewport.destroy();
            }
            overlay.layer_surface.destroy();
            overlay.surface.destroy();
        }

        idle_log::info!("wayland-present: overlay hidden");
    }

    pub fn dismiss_from_input(&mut self) {
        if !self.visible.load(Ordering::SeqCst) {
            return;
        }
        if self
            .dismiss_grace_until
            .is_some_and(|deadline| Instant::now() < deadline)
        {
            return;
        }
        idle_log::info!("wayland-present: dismissed by user input");
        self.hide();
    }
}
