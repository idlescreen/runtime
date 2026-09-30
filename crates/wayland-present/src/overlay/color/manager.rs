// SPDX-License-Identifier: MIT

//! `wp_color_manager_v1` global interface handler and capabilities state.

use std::collections::HashMap;

use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_surface_v1, wp_color_manager_v1, wp_image_description_v1,
};

pub use wp_color_manager_v1::{Feature, Primaries, RenderIntent, TransferFunction};

use crate::overlay::state::SessionState;

/// Tracks compositor color management capabilities and active surface bindings.
#[derive(Debug, Default)]
pub struct ColorManagementState {
    pub supported_features: Vec<Feature>,
    pub supported_intents: Vec<RenderIntent>,
    pub supported_tfs: Vec<TransferFunction>,
    pub supported_primaries: Vec<Primaries>,
    pub done: bool,
    pub is_hdr_ready: bool,
    pub surface_color: HashMap<u32, wp_color_management_surface_v1::WpColorManagementSurfaceV1>,
    pub image_descriptions: HashMap<u32, wp_image_description_v1::WpImageDescriptionV1>,
    pub pending_surfaces: HashMap<u32, wp_color_management_surface_v1::WpColorManagementSurfaceV1>,
}

impl ColorManagementState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn supports_feature(&self, feature: Feature) -> bool {
        self.supported_features.contains(&feature)
    }

    #[must_use]
    pub fn supports_tf(&self, tf: TransferFunction) -> bool {
        self.supported_tfs.contains(&tf)
    }

    #[must_use]
    pub fn supports_primaries(&self, primaries: Primaries) -> bool {
        self.supported_primaries.contains(&primaries)
    }

    /// Returns true if the compositor advertises necessary features for HDR10.
    #[must_use]
    pub fn supports_hdr10(&self) -> bool {
        self.supports_feature(Feature::WindowsBt2100)
            || (self.supports_feature(Feature::Parametric)
                && self.supports_primaries(Primaries::Bt2020)
                && self.supports_tf(TransferFunction::St2084Pq))
    }

    /// Attaches a confirmed ready image description to the pending surface.
    pub fn apply_ready_description(
        &mut self,
        output_id: u32,
        desc: &wp_image_description_v1::WpImageDescriptionV1,
    ) {
        if let Some(surf) = self.pending_surfaces.remove(&output_id) {
            surf.set_image_description(desc, RenderIntent::Perceptual);
            self.surface_color.insert(output_id, surf);
            self.image_descriptions.insert(output_id, desc.clone());
            self.is_hdr_ready = true;
            idle_log::info!(
                output_id,
                "wayland-present: applied HDR10 image description to surface"
            );
        }
    }

    /// Removes pending registrations if image description creation failed.
    pub fn handle_description_failed(&mut self, output_id: u32) {
        self.pending_surfaces.remove(&output_id);
        self.surface_color.remove(&output_id);
        self.image_descriptions.remove(&output_id);
        self.is_hdr_ready = false;
    }
}

impl Dispatch<wp_color_manager_v1::WpColorManagerV1, ()> for SessionState {
    fn event(
        state: &mut Self,
        _: &wp_color_manager_v1::WpColorManagerV1,
        event: wp_color_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_color_manager_v1::Event::SupportedIntent {
                render_intent: WEnum::Value(intent),
            } => {
                state.color_state.supported_intents.push(intent);
            }
            wp_color_manager_v1::Event::SupportedFeature {
                feature: WEnum::Value(feat),
            } => {
                state.color_state.supported_features.push(feat);
            }
            wp_color_manager_v1::Event::SupportedTfNamed {
                tf: WEnum::Value(val),
            } => {
                state.color_state.supported_tfs.push(val);
            }
            wp_color_manager_v1::Event::SupportedPrimariesNamed {
                primaries: WEnum::Value(prim),
            } => {
                state.color_state.supported_primaries.push(prim);
            }
            wp_color_manager_v1::Event::Done => {
                state.color_state.done = true;
                idle_log::info!(
                    hdr10 = state.color_state.supports_hdr10(),
                    features = state.color_state.supported_features.len(),
                    "wayland-present: wp_color_manager_v1 feature discovery complete"
                );
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_color_management_surface_v1::WpColorManagementSurfaceV1, u32> for SessionState {
    fn event(
        _: &mut Self,
        _: &wp_color_management_surface_v1::WpColorManagementSurfaceV1,
        _: wp_color_management_surface_v1::Event,
        _: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
