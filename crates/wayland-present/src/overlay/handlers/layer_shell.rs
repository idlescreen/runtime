// SPDX-License-Identifier: MIT

use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use super::super::state::SessionState;

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for SessionState {
    fn event(
        _: &mut Self,
        _: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        _: zwlr_layer_shell_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, u32> for SessionState {
    fn event(
        state: &mut Self,
        _: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        output_id: &u32,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                state.configure_overlay(*output_id, serial, width, height);
            }
            zwlr_layer_surface_v1::Event::Closed => {
                state.remove_overlay(*output_id);
            }
            _ => {}
        }

        let _ = queue;
    }
}
