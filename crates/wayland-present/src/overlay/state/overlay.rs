// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use super::types::SessionState;

impl SessionState {
    pub fn create_overlay(&mut self, output_id: u32) {
        if self.overlays.contains_key(&output_id) {
            self.remove_overlay(output_id);
        }

        let Some(compositor) = &self.compositor else {
            idle_log::warn!("wayland-present: missing compositor");
            return;
        };

        let output = self
            .outputs
            .iter()
            .find(|target| target.id == output_id)
            .map(|target| &target.output);
        let Some(output) = output else {
            return;
        };

        let surface = compositor.create_surface(&self.queue, output_id);
        let role = if let Some(layer_shell) = &self.layer_shell {
            let layer_surface = layer_shell.get_layer_surface(
                &surface,
                Some(output),
                zwlr_layer_shell_v1::Layer::Overlay,
                "idlescreen".to_string(),
                &self.queue,
                output_id,
            );
            let anchor = zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right;
            layer_surface.set_anchor(anchor);
            layer_surface.set_exclusive_zone(Self::exclusive_zone_for(self.screensaver_mode));
            layer_surface.set_margin(0, 0, 0, 0);
            layer_surface.set_keyboard_interactivity(
                zwlr_layer_surface_v1::KeyboardInteractivity::OnDemand,
            );
            layer_surface.set_size(0, 0);
            surface.commit();
            super::types::OverlayRole::Layer(layer_surface)
        } else if let Some(xdg_wm_base) = &self.xdg_wm_base {
            let xdg_surface = xdg_wm_base.get_xdg_surface(&surface, &self.queue, output_id);
            let toplevel = xdg_surface.get_toplevel(&self.queue, output_id);
            toplevel.set_title("idlescreen".to_string());
            toplevel.set_app_id("idlescreen".to_string());
            toplevel.set_fullscreen(Some(output));
            surface.commit();
            super::types::OverlayRole::Xdg { xdg_surface, toplevel }
        } else {
            idle_log::warn!("wayland-present: missing layer shell and xdg_wm_base");
            return;
        };

        let viewport = if std::env::var_os("IDLE_HW_VIEWPORT").is_some() {
            self.viewporter.as_ref().map(|vp| vp.get_viewport(&surface, &self.queue, ()))
        } else {
            None
        };

        self.overlays.insert(
            output_id,
            super::types::MonitorOverlay {
                surface: surface.clone(),
                role,
                width: 0,
                height: 0,
                buffers: [None, None],
                current_buffer: 0,
                viewport,
            },
        );

        if let Some(color_mgr) = &self.color_manager {
            crate::overlay::color::configure_hdr_overlay(
                &mut self.color_state,
                color_mgr,
                &surface,
                output_id,
                &self.queue,
                &crate::overlay::color::HdrConfig::default(),
            );
        }
    }

    #[allow(clippy::cast_possible_wrap)]
    pub fn configure_overlay(&mut self, output_id: u32, serial: u32, width: u32, height: u32) {
        let fullscreen = self.screensaver_mode;
        let (render_w, render_h) = {
            let Some(overlay) = self.overlays.get_mut(&output_id) else {
                return;
            };

            if let super::types::OverlayRole::Layer(layer) = &overlay.role {
                layer.set_exclusive_zone(Self::exclusive_zone_for(fullscreen));
                Self::apply_tiling_margins(
                    layer,
                    &overlay.surface,
                    output_id,
                    width,
                    height,
                    &self.output_mode_size,
                    fullscreen,
                );
                layer.ack_configure(serial);
            }
            let (render_w, render_h) = Self::render_dimensions(
                output_id,
                width,
                height,
                &self.output_mode_size,
                fullscreen,
            );
            overlay.width = render_w;
            overlay.height = render_h;

            Self::apply_opaque_region(
                self.compositor.as_ref(),
                &overlay.surface,
                &self.queue,
                render_w as i32,
                render_h as i32,
            );

            if let Some(viewport) = &overlay.viewport
                && render_w > 0
                && render_h > 0
            {
                viewport.set_destination(render_w as i32, render_h as i32);
            }
            (render_w, render_h)
        };

        self.register_configured_output(output_id, render_w, render_h);
        if fullscreen {
            idle_log::info!(
                output_id,
                configured_w = width,
                configured_h = height,
                render_w,
                render_h,
                "wayland-present: fullscreen saver geometry (covers panel when mode > configure)"
            );
        }

        if self.screensaver_mode {
            // Frames arrive via update_frame; null commit completes configure.
            if let Some(overlay) = self.overlays.get(&output_id) {
                overlay.surface.commit();
            }
            return;
        }
        self.attach_solid_buffer(output_id, render_w, render_h);
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn update_frame(&mut self, output_id: u32, width: u32, height: u32, pixels: &[u8]) {
        if !self.screensaver_mode {
            return;
        }

        let Some(shm) = &self.shm else {
            return;
        };

        let Some(overlay) = self.overlays.get_mut(&output_id) else {
            return;
        };

        // Layer-shell protocol: do not attach a buffer until the first
        // `configure` has been acked. Committing early is a protocol error and
        // disconnects the client (seen as "failed to read Wayland events").
        if overlay.width == 0 || overlay.height == 0 {
            return;
        }

        let prev_idx = overlay.current_buffer;
        let damage = if let Some(ref prev_buf) = overlay.buffers[prev_idx] {
            if prev_buf.width() == width && prev_buf.height() == height {
                super::super::damage::compute_damage_with_threshold(
                    prev_buf.as_slice(),
                    pixels,
                    width,
                    height,
                )
            } else {
                Some(super::super::damage::DamageRect::full(width, height))
            }
        } else {
            Some(super::super::damage::DamageRect::full(width, height))
        };

        overlay.current_buffer ^= 1;

        if !super::super::buffer::ensure_frame_buffer(
            &mut overlay.buffers[overlay.current_buffer],
            shm,
            &self.queue,
            width,
            height,
            pixels,
        ) {
            return;
        }

        let queue = self.queue.clone();
        if !Self::commit_frame_buffer(
            &queue,
            overlay,
            width,
            height,
            damage,
            self.presentation.as_ref(),
        ) {
            idle_log::error!(
                output_id,
                "wayland-present: frame buffer missing after ensure; skipping frame"
            );
            return;
        }
        // Frame committed: notify frame signal to pace render loop.
        self.frame_signal.notify();
    }

    pub fn remove_overlay(&mut self, output_id: u32) {
        self.color_state.handle_description_failed(output_id);
        if let Some(overlay) = self.overlays.remove(&output_id) {
            if let Some(viewport) = overlay.viewport {
                viewport.destroy();
            }
            match overlay.role {
                super::types::OverlayRole::Layer(layer) => layer.destroy(),
                super::types::OverlayRole::Xdg {
                    toplevel,
                    xdg_surface,
                } => {
                    toplevel.destroy();
                    xdg_surface.destroy();
                }
            }
            overlay.surface.destroy();
        }
    }
}
