// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn validate_idle_timeout_bounds() {
    assert!(validate_idle_timeout(0).is_err());
    assert!(validate_idle_timeout(241).is_err());
    assert!(validate_idle_timeout(1).is_ok());
    assert!(validate_idle_timeout(240).is_ok());
    assert!(validate_idle_timeout(120).is_ok());
}

#[test]
fn validate_render_scale_in_range() {
    assert!(validate_render_scale(0.25).is_ok());
    assert!(validate_render_scale(1.0).is_ok());
    assert!(validate_render_scale(0.5).is_ok());
    assert!(validate_render_scale(0.24).is_err());
    assert!(validate_render_scale(1.01).is_err());
    assert!(validate_render_scale(f32::NAN).is_err());
}

#[test]
fn normalize_render_scale_handles_edges() {
    assert!(normalize_render_scale(None).expect("None ok").is_none());
    assert!(normalize_render_scale(Some(0.0)).expect("0.0 ok").is_none());
    assert!(
        normalize_render_scale(Some(-1.0))
            .expect("-1.0 ok")
            .is_none()
    );
    assert_eq!(
        normalize_render_scale(Some(0.5)).expect("0.5 ok"),
        Some(0.5)
    );
    assert!(normalize_render_scale(Some(2.0)).is_err());
}

#[test]
fn validate_saver_choice_rejects_path_traversal() {
    assert!(validate_saver_choice(Some("../evil")).is_err());
    assert!(validate_saver_choice(Some("beams;rm")).is_err());
}

#[test]
fn validate_saver_choice_rejects_absolute_path() {
    assert!(validate_saver_choice(Some("/bin/sh")).is_err());
}

#[test]
fn validate_saver_choice_rejects_null_bytes() {
    assert!(validate_saver_choice(Some("beams\0")).is_err());
}
