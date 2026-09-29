use super::*;

/// Serializes tests that mutate process env vars — parallel test threads
/// otherwise race and flake (observed: simulation_tick_hz clamp test).
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn resolve_render_scale_clamps_high() {
    let s = resolve_render_scale(Some(2.0));
    assert!(s <= 1.0);
}

#[test]
fn resolve_render_scale_clamps_low() {
    let s = resolve_render_scale(Some(0.1));
    assert!(s >= 0.25);
}

#[test]
fn resolve_render_scale_default_cpu() {
    let s = resolve_render_scale(None);
    assert!(s > 0.0 && s <= 1.0);
}

#[test]
fn render_scale_in_range() {
    let s = render_scale();
    assert!(s > 0.0 && s <= 1.0);
}

#[test]
fn filter_mode_from_name_recognizes_nearest() {
    // Pure — no env (env tests race under cargo test parallel).
    assert!(matches!(
        FilterMode::from_name("nearest"),
        FilterMode::Nearest
    ));
    assert!(matches!(
        FilterMode::from_name("NEAREST"),
        FilterMode::Nearest
    ));
    assert!(matches!(
        FilterMode::from_name("point"),
        FilterMode::Nearest
    ));
}

#[test]
fn filter_mode_from_name_defaults_to_linear() {
    assert!(matches!(FilterMode::from_name(""), FilterMode::Linear));
    assert!(matches!(
        FilterMode::from_name("linear"),
        FilterMode::Linear
    ));
}

#[test]
fn filter_mode_from_name_unknown_falls_back_to_linear() {
    assert!(matches!(FilterMode::from_name("bogus"), FilterMode::Linear));
}

#[test]
fn max_fps_zero_when_unset() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("IDLE_MAX_FPS").ok();
    unsafe {
        std::env::remove_var("IDLE_MAX_FPS");
    }
    assert_eq!(max_fps(), 0);
    if let Some(v) = prior {
        unsafe {
            std::env::set_var("IDLE_MAX_FPS", v);
        }
    }
}

#[test]
fn simulation_tick_hz_default_in_range() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("IDLE_TICK_HZ").ok();
    unsafe {
        std::env::remove_var("IDLE_TICK_HZ");
    }
    let hz = simulation_tick_hz();
    assert!((15.0..=240.0).contains(&hz));
    if let Some(v) = prior {
        unsafe {
            std::env::set_var("IDLE_TICK_HZ", v);
        }
    }
}

#[test]
fn target_fps_matches_detected_when_unset() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let detected = 144;
    let prior = std::env::var("IDLE_MAX_FPS").ok();
    unsafe {
        std::env::remove_var("IDLE_MAX_FPS");
    }
    let fps = target_fps(detected);
    assert!((fps - detected as f32).abs() < f32::EPSILON);
    if let Some(v) = prior {
        unsafe {
            std::env::set_var("IDLE_MAX_FPS", v);
        }
    }
}

#[test]
fn target_fps_floors_detected_at_60() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("IDLE_MAX_FPS").ok();
    unsafe {
        std::env::remove_var("IDLE_MAX_FPS");
    }
    assert!((target_fps(30) - (60.0)).abs() < 1e-3);
    assert!((target_fps(0) - (60.0)).abs() < 1e-3);
    assert!((target_fps(60) - (60.0)).abs() < 1e-3);
    restore_max_fps(prior);
}

#[test]
fn target_fps_respects_max_cap() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("IDLE_MAX_FPS").ok();
    unsafe {
        std::env::set_var("IDLE_MAX_FPS", "90");
    }
    assert!((target_fps(144) - (90.0)).abs() < 1e-3);
    assert!((target_fps(60) - (60.0)).abs() < 1e-3);
    restore_max_fps(prior);
}

#[test]
fn simulation_tick_hz_clamps_env_outliers() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("IDLE_TICK_HZ").ok();
    unsafe {
        std::env::set_var("IDLE_TICK_HZ", "1");
    }
    assert!((simulation_tick_hz() - (15.0)).abs() < 1e-3);
    unsafe {
        std::env::set_var("IDLE_TICK_HZ", "9999");
    }
    assert!((simulation_tick_hz() - (240.0)).abs() < 1e-3);
    match prior {
        Some(v) => unsafe {
            std::env::set_var("IDLE_TICK_HZ", v);
        },
        None => unsafe {
            std::env::remove_var("IDLE_TICK_HZ");
        },
    }
}

fn restore_max_fps(prior: Option<String>) {
    match prior {
        Some(v) => unsafe {
            std::env::set_var("IDLE_MAX_FPS", v);
        },
        None => unsafe {
            std::env::remove_var("IDLE_MAX_FPS");
        },
    }
}

#[test]
fn frame_upscaler_never_uses_gpu() {
    let upscaler = FrameUpscaler::new(FilterMode::Linear);
    assert!(!upscaler.using_gpu());
    assert_eq!(upscaler.adapter_name(), None);
}
