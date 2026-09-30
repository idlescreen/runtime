#![allow(clippy::float_cmp)]

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

#[test]
fn power_throttling_on_battery_clamps_fps_and_tick() {
    let (fps, tick) = super::apply_power_throttling(60.0, 60.0, true);
    assert!((fps - 30.0).abs() < 1e-3);
    assert!((tick - 30.0).abs() < 1e-3);

    let (fps_low, tick_low) = super::apply_power_throttling(20.0, 10.0, true);
    assert!((fps_low - 20.0).abs() < 1e-3);
    assert!((tick_low - 15.0).abs() < 1e-3);
}

#[test]
fn power_throttling_on_ac_restores_nominal() {
    let (fps, tick) = super::apply_power_throttling(144.0, 60.0, false);
    assert!((fps - 144.0).abs() < 1e-3);
    assert!((tick - 60.0).abs() < 1e-3);
}

#[test]
fn power_throttling_high_refresh_battery_and_ac() {
    // 240Hz gaming display on battery clamps to 30 FPS / 30 Hz
    let (fps_bat, tick_bat) = super::apply_power_throttling(240.0, 60.0, true);
    assert!((fps_bat - 30.0).abs() < 1e-3);
    assert!((tick_bat - 30.0).abs() < 1e-3);

    // On AC, restored to full 240 FPS / 60 Hz nominal tick
    let (fps_ac, tick_ac) = super::apply_power_throttling(240.0, 60.0, false);
    assert!((fps_ac - 240.0).abs() < 1e-3);
    assert!((tick_ac - 60.0).abs() < 1e-3);
}

#[test]
fn dynamic_battery_cache_integration() {
    crate::daemon::power::battery::set_cached_on_battery(true);
    assert!(crate::daemon::battery::is_on_battery());
    let (fps, tick) =
        super::apply_power_throttling(60.0, 60.0, crate::daemon::battery::is_on_battery());
    assert!((fps - 30.0).abs() < 1e-3);
    assert!((tick - 30.0).abs() < 1e-3);

    crate::daemon::power::battery::set_cached_on_battery(false);
    assert!(!crate::daemon::battery::is_on_battery());
    let (fps, tick) =
        super::apply_power_throttling(60.0, 60.0, crate::daemon::battery::is_on_battery());
    assert!((fps - 60.0).abs() < 1e-3);
    assert!((tick - 60.0).abs() < 1e-3);

    crate::daemon::power::battery::reset_cached_on_battery();
}

#[test]
fn adaptive_pacing_transitions_stages() {
    use super::{AdaptivePacingStage, resolve_adaptive_pacing};

    // Stage 1: Interactive (elapsed < 5s)
    let (fps_i, tick_i, stage_i) = resolve_adaptive_pacing(144.0, Duration::from_secs(2), false);
    assert_eq!(stage_i, AdaptivePacingStage::Interactive);
    assert_eq!(fps_i, 144.0);
    assert_eq!(tick_i, 60.0);

    // Stage 2: Active (5s <= elapsed < 45s)
    let (fps_a, tick_a, stage_a) = resolve_adaptive_pacing(144.0, Duration::from_secs(15), false);
    assert_eq!(stage_a, AdaptivePacingStage::Active);
    assert_eq!(fps_a, 60.0);
    assert_eq!(tick_a, 60.0);

    // Stage 3: Deep Ambient (elapsed >= 45s)
    let (fps_d, tick_d, stage_d) = resolve_adaptive_pacing(60.0, Duration::from_mins(1), false);
    assert_eq!(stage_d, AdaptivePacingStage::DeepAmbient);
    assert_eq!(fps_d, 30.0);
    assert_eq!(tick_d, 30.0);

    // Stage 3: Deep Ambient on 24Hz-aligned display
    let (fps_24, tick_24, stage_24) = resolve_adaptive_pacing(48.0, Duration::from_mins(1), false);
    assert_eq!(stage_24, AdaptivePacingStage::DeepAmbient);
    assert_eq!(fps_24, 24.0);
    assert_eq!(tick_24, 30.0);

    // Stage 4: Battery (immediate cap regardless of elapsed)
    let (fps_b, tick_b, stage_b) = resolve_adaptive_pacing(144.0, Duration::from_secs(2), true);
    assert_eq!(stage_b, AdaptivePacingStage::Battery);
    assert_eq!(fps_b, 30.0);
    assert_eq!(tick_b, 30.0);
}
