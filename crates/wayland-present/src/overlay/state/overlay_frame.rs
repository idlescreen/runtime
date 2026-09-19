// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Frame and solid-buffer attachment for monitor overlays.

use crate::output::OutputLayout;

use super::types::{MonitorOverlay, SessionState};

/// Safe geometry for attaching a frame (and optional viewport destination).
///
/// Zero-size destination or buffer is a Wayland protocol error and has been
/// observed to disconnect the client (killing the presenter thread).
pub fn frame_geometry_ok(buffer_w: u32, buffer_h: u32, dst_w: u32, dst_h: u32) -> bool {
    buffer_w > 0 && buffer_h > 0 && dst_w > 0 && dst_h > 0
}

/// Layer surface has received (and applied) its first configure.
///
/// Until this is true, attaching a buffer is a protocol error.
pub fn layer_surface_configured(surface_w: u32, surface_h: u32) -> bool {
    surface_w > 0 && surface_h > 0
}

impl SessionState {
    /// Publish configured size into the output registry used by presenters.
    pub(super) fn register_configured_output(
        &mut self,
        output_id: u32,
        render_w: u32,
        render_h: u32,
    ) {
        let refresh_rate_hz = self
            .output_refresh_hz
            .get(&output_id)
            .copied()
            .unwrap_or(60);
        let (x, y) = self
            .output_origin
            .get(&output_id)
            .copied()
            .unwrap_or((0, 0));
        let scale = self.output_scale.get(&output_id).copied().unwrap_or(1);
        self.output_registry.upsert(OutputLayout {
            id: output_id,
            width: render_w,
            height: render_h,
            refresh_rate_hz,
            x,
            y,
            scale,
        });
    }

    /// Attach a solid-color buffer for non-screensaver overlay mode.
    #[allow(clippy::cast_possible_wrap)]
    pub(super) fn attach_solid_buffer(&mut self, output_id: u32, render_w: u32, render_h: u32) {
        let Some(appearance) = self.appearance else {
            return;
        };
        let Some(shm) = &self.shm else {
            return;
        };

        let buffer = super::super::buffer::create_solid_buffer(
            shm,
            &self.queue,
            render_w,
            render_h,
            appearance.color,
        );

        let Some(overlay) = self.overlays.get_mut(&output_id) else {
            return;
        };
        overlay.buffers[0] = buffer;
        overlay.current_buffer = 0;

        if let Some(buffer) = &overlay.buffers[0] {
            overlay.surface.attach(Some(&buffer.wl_buffer), 0, 0);
            overlay
                .surface
                .damage_buffer(0, 0, render_w as i32, render_h as i32);
            overlay.surface.commit();
        }
    }

    /// Attach a screensaver frame buffer after `ensure_frame_buffer` succeeds.
    #[allow(clippy::cast_possible_wrap)]
    pub(super) fn commit_frame_buffer(
        queue: &wayland_client::QueueHandle<SessionState>,
        overlay: &mut MonitorOverlay,
        width: u32,
        height: u32,
    ) -> bool {
        let Some(buffer) = overlay.buffers[overlay.current_buffer].as_ref() else {
            return false;
        };

        // Prefer configured surface size; never invent a destination for an
        // unconfigured layer surface (would race the first configure).
        if !layer_surface_configured(overlay.width, overlay.height) {
            idle_log::debug!(
                buffer_w = width,
                buffer_h = height,
                "wayland-present: skip frame — layer surface not configured yet"
            );
            return false;
        }
        let dst_w = overlay.width;
        let dst_h = overlay.height;

        if !frame_geometry_ok(width, height, dst_w, dst_h) {
            idle_log::error!(
                buffer_w = width,
                buffer_h = height,
                dst_w,
                dst_h,
                "wayland-present: refuse frame with zero geometry (protocol risk)"
            );
            return false;
        }

        // Never set_destination(0,0) — protocol error / client disconnect.
        if let Some(viewport) = &overlay.viewport {
            viewport.set_destination(dst_w as i32, dst_h as i32);
        }

        idle_log::debug!(
            buffer_w = width,
            buffer_h = height,
            surface_w = dst_w,
            surface_h = dst_h,
            has_viewport = overlay.viewport.is_some(),
            "wayland-present: commit frame"
        );

        overlay.surface.attach(Some(&buffer.wl_buffer), 0, 0);
        overlay
            .surface
            .damage_buffer(0, 0, width as i32, height as i32);

        // Request frame callback to wake up `poll()` on VSync, enabling backpressure.
        let _ = overlay.surface.frame(queue, ());

        overlay.surface.commit();
        true
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::{frame_geometry_ok, layer_surface_configured};

    #[test]
    fn rejects_zero_buffer() {
        assert!(!frame_geometry_ok(0, 1080, 1920, 1080));
        assert!(!frame_geometry_ok(1920, 0, 1920, 1080));
    }

    #[test]
    fn rejects_zero_destination() {
        assert!(!frame_geometry_ok(1920, 1080, 0, 1080));
        assert!(!frame_geometry_ok(1920, 1080, 1920, 0));
    }

    #[test]
    fn accepts_positive() {
        assert!(frame_geometry_ok(960, 540, 1920, 1080));
        assert!(frame_geometry_ok(1920, 1080, 1920, 1080));
    }

    #[test]
    fn layer_not_configured_until_positive_size() {
        assert!(!layer_surface_configured(0, 0));
        assert!(!layer_surface_configured(1920, 0));
        assert!(!layer_surface_configured(0, 1080));
        assert!(layer_surface_configured(1920, 1080));
    }
}
