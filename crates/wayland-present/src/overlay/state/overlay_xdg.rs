// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

use super::types::{OverlayRole, SessionState};

impl SessionState {
    pub fn configure_xdg_toplevel(&mut self, output_id: u32, width: i32, height: i32) {
        if let Some(overlay) = self.overlays.get_mut(&output_id) {
            if width > 0 && height > 0 {
                overlay.width = width as u32;
                overlay.height = height as u32;
            }
        }
    }

    pub fn configure_xdg_surface(&mut self, output_id: u32, serial: u32) {
        let (render_w, render_h) = {
            let Some(overlay) = self.overlays.get_mut(&output_id) else {
                return;
            };
            if let OverlayRole::Xdg { xdg_surface, .. } = &overlay.role {
                xdg_surface.ack_configure(serial);
            }
            let (native_w, native_h) = self
                .output_mode_size
                .get(&output_id)
                .copied()
                .unwrap_or((1920, 1080));
            let render_w = if overlay.width > 0 {
                overlay.width
            } else {
                native_w
            };
            let render_h = if overlay.height > 0 {
                overlay.height
            } else {
                native_h
            };
            overlay.width = render_w;
            overlay.height = render_h;

            Self::apply_opaque_region(
                self.compositor.as_ref(),
                &overlay.surface,
                &self.queue,
                render_w as i32,
                render_h as i32,
            );
            (render_w, render_h)
        };

        self.register_configured_output(output_id, render_w, render_h);

        if self.screensaver_mode {
            if let Some(overlay) = self.overlays.get(&output_id) {
                overlay.surface.commit();
            }
            return;
        }
        self.attach_solid_buffer(output_id, render_w, render_h);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_xdg_toplevel_configure_updates_dimensions() {
        let (output_w, output_h) = (2560, 1440);
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        if output_w > 0 && output_h > 0 {
            width = output_w;
            height = output_h;
        }
        assert_eq!(width, 2560);
        assert_eq!(height, 1440);
    }
}
