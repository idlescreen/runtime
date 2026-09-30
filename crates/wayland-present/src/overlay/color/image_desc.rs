// SPDX-License-Identifier: MIT

//! HDR10 image description creation and surface parameter configuration.

use wayland_client::protocol::wl_surface;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_manager_v1, wp_image_description_creator_params_v1, wp_image_description_v1,
};

use super::manager::{ColorManagementState, Feature, Primaries, TransferFunction};
use crate::overlay::state::SessionState;

/// Target luminance parameters for HDR10 mastering and reference display.
#[derive(Debug, Clone)]
pub struct HdrConfig {
    pub min_luminance: f32,
    pub max_luminance: u32,
    pub reference_white: u32,
}

impl Default for HdrConfig {
    fn default() -> Self {
        Self {
            min_luminance: 0.005,
            max_luminance: 1000,
            reference_white: 203,
        }
    }
}

/// Attempts to request HDR10 color description creation for an overlay surface.
pub fn configure_hdr_overlay(
    color_state: &mut ColorManagementState,
    color_manager: &wp_color_manager_v1::WpColorManagerV1,
    surface: &wl_surface::WlSurface,
    output_id: u32,
    queue: &QueueHandle<SessionState>,
    config: &HdrConfig,
) -> bool {
    if !color_state.supports_hdr10() {
        return false;
    }

    let color_surface = color_manager.get_surface(surface, queue, output_id);

    let desc = if color_state.supports_feature(Feature::WindowsBt2100) {
        color_manager.create_windows_bt2100(queue, output_id)
    } else if color_state.supports_feature(Feature::Parametric) {
        let creator = color_manager.create_parametric_creator(queue, output_id);
        creator.set_primaries_named(Primaries::Bt2020);
        creator.set_tf_named(TransferFunction::St2084Pq);
        if color_state.supports_feature(Feature::SetLuminances) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let min_lum_scaled = (config.min_luminance * 10_000.0) as u32;
            creator.set_luminances(min_lum_scaled, config.max_luminance, config.reference_white);
        }
        creator.create(queue, output_id)
    } else {
        return false;
    };

    color_state
        .pending_surfaces
        .insert(output_id, color_surface);
    color_state.image_descriptions.insert(output_id, desc);
    true
}

impl Dispatch<wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1, u32>
    for SessionState
{
    fn event(
        _: &mut Self,
        _: &wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1,
        _: wp_image_description_creator_params_v1::Event,
        _: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_image_description_v1::WpImageDescriptionV1, u32> for SessionState {
    fn event(
        state: &mut Self,
        desc: &wp_image_description_v1::WpImageDescriptionV1,
        event: wp_image_description_v1::Event,
        &output_id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_image_description_v1::Event::Ready { identity } => {
                idle_log::info!(
                    output_id,
                    identity,
                    "wayland-present: HDR image description ready"
                );
                state.color_state.apply_ready_description(output_id, desc);
            }
            wp_image_description_v1::Event::Ready2 {
                identity_hi,
                identity_lo,
            } => {
                idle_log::info!(
                    output_id,
                    identity_hi,
                    identity_lo,
                    "wayland-present: HDR image description ready2"
                );
                state.color_state.apply_ready_description(output_id, desc);
            }
            wp_image_description_v1::Event::Failed { cause, msg } => {
                idle_log::warn!(
                    output_id,
                    ?cause,
                    msg = %msg,
                    "wayland-present: HDR image description creation failed"
                );
                state.color_state.handle_description_failed(output_id);
            }
            _ => {}
        }
    }
}
