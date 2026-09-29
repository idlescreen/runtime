// SPDX-License-Identifier: Apache-2.0

use super::{clamp_present_fps, clamp_tick_hz};
use std::time::Duration;

#[test]
fn clamp_present_fps_rejects_non_finite() {
    assert!((clamp_present_fps(f32::NAN) - (60.0)).abs() < 1e-3);
    assert!((clamp_present_fps(f32::INFINITY) - (60.0)).abs() < 1e-3);
    assert!((clamp_present_fps(f32::NEG_INFINITY) - (60.0)).abs() < 1e-3);
}

#[test]
fn clamp_present_fps_rejects_zero_and_negative() {
    assert!((clamp_present_fps(0.0) - (60.0)).abs() < 1e-3);
    assert!((clamp_present_fps(-1.0) - (60.0)).abs() < 1e-3);
    assert!((clamp_present_fps(-0.0) - (60.0)).abs() < 1e-3);
}

#[test]
fn clamp_present_fps_clamps_to_band() {
    assert!((clamp_present_fps(0.5) - (1.0)).abs() < 1e-3);
    assert!((clamp_present_fps(1.0) - (1.0)).abs() < 1e-3);
    assert!((clamp_present_fps(60.0) - (60.0)).abs() < 1e-3);
    assert!((clamp_present_fps(480.0) - (480.0)).abs() < 1e-3);
    assert!((clamp_present_fps(1000.0) - (480.0)).abs() < 1e-3);
}

#[test]
fn clamp_tick_hz_rejects_non_finite() {
    assert!((clamp_tick_hz(f32::NAN) - (60.0)).abs() < 1e-3);
    assert!((clamp_tick_hz(f32::INFINITY) - (60.0)).abs() < 1e-3);
}

#[test]
fn clamp_tick_hz_rejects_zero_and_negative() {
    assert!((clamp_tick_hz(0.0) - (60.0)).abs() < 1e-3);
    assert!((clamp_tick_hz(-30.0) - (60.0)).abs() < 1e-3);
}

#[test]
fn clamp_tick_hz_clamps_to_band() {
    assert!((clamp_tick_hz(1.0) - (15.0)).abs() < 1e-3);
    assert!((clamp_tick_hz(14.9) - (15.0)).abs() < 1e-3);
    assert!((clamp_tick_hz(15.0) - (15.0)).abs() < 1e-3);
    assert!((clamp_tick_hz(60.0) - (60.0)).abs() < 1e-3);
    assert!((clamp_tick_hz(240.0) - (240.0)).abs() < 1e-3);
    assert!((clamp_tick_hz(500.0) - (240.0)).abs() < 1e-3);
}

#[test]
fn clamped_present_fps_yields_finite_frame_duration() {
    for raw in [f32::NAN, 0.0, -1.0, 0.001, 1.0, 60.0, 480.0, 10_000.0] {
        let fps = clamp_present_fps(raw);
        assert!(fps.is_finite() && fps > 0.0, "fps={fps} from raw={raw}");
        let d = Duration::from_secs_f32(1.0 / fps);
        assert!(d > Duration::ZERO);
        assert!(d < Duration::from_secs(2));
    }
}
