// SPDX-License-Identifier: MIT

use super::image_desc::HdrConfig;
use super::manager::{ColorManagementState, Feature, Primaries, RenderIntent, TransferFunction};

#[test]
fn test_color_state_default() {
    let state = ColorManagementState::new();
    assert!(!state.done);
    assert!(!state.is_hdr_ready);
    assert!(!state.supports_hdr10());
    assert!(state.supported_features.is_empty());
    assert!(state.supported_tfs.is_empty());
    assert!(state.supported_primaries.is_empty());
}

#[test]
fn test_supports_hdr10_windows_bt2100() {
    let mut state = ColorManagementState::new();
    state.supported_features.push(Feature::WindowsBt2100);
    assert!(state.supports_hdr10());
}

#[test]
fn test_supports_hdr10_parametric_bt2020_pq() {
    let mut state = ColorManagementState::new();
    state.supported_features.push(Feature::Parametric);
    assert!(!state.supports_hdr10());

    state.supported_primaries.push(Primaries::Bt2020);
    assert!(!state.supports_hdr10());

    state.supported_tfs.push(TransferFunction::St2084Pq);
    assert!(state.supports_hdr10());
}

#[test]
fn test_supports_hdr10_missing_primaries() {
    let mut state = ColorManagementState::new();
    state.supported_features.push(Feature::Parametric);
    state.supported_primaries.push(Primaries::Srgb);
    state.supported_tfs.push(TransferFunction::St2084Pq);
    assert!(!state.supports_hdr10());
}

#[test]
fn test_supports_hdr10_missing_transfer_function() {
    let mut state = ColorManagementState::new();
    state.supported_features.push(Feature::Parametric);
    state.supported_primaries.push(Primaries::Bt2020);
    state.supported_tfs.push(TransferFunction::Bt1886);
    assert!(!state.supports_hdr10());
}

#[test]
fn test_hdr_config_defaults_and_scaling() {
    let config = HdrConfig::default();
    assert!((config.min_luminance - 0.005).abs() < f32::EPSILON);
    assert_eq!(config.max_luminance, 1000);
    assert_eq!(config.reference_white, 203);

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scaled_min = (config.min_luminance * 10_000.0) as u32;
    assert_eq!(scaled_min, 50);
}

#[test]
fn test_handle_description_failed() {
    let mut state = ColorManagementState::new();
    state.is_hdr_ready = true;
    state.handle_description_failed(42);
    assert!(!state.is_hdr_ready);
    assert!(!state.pending_surfaces.contains_key(&42));
    assert!(!state.surface_color.contains_key(&42));
    assert!(!state.image_descriptions.contains_key(&42));
}

#[test]
fn test_feature_and_intent_queries() {
    let mut state = ColorManagementState::new();
    state.supported_features.push(Feature::SetLuminances);
    state.supported_intents.push(RenderIntent::Perceptual);

    assert!(state.supports_feature(Feature::SetLuminances));
    assert!(!state.supports_feature(Feature::SetTfPower));
    assert_eq!(state.supported_intents.len(), 1);
}
