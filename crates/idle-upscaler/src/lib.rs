// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! CPU upscaling for trance screensaver frames.
//!
//! **Note (2026):** This crate is named `idle-upscaler` (renamed from
//! `trance-gpu` in this release). The historical name implied GPU
//! acceleration, but the implementation is always CPU-based — see
//! [`gpu_enabled`] which unconditionally returns `false`. The rename
//! makes the actual behavior unambiguous.
//!
//! Two paths exist for upscaling a low-resolution simulation grid to the
//! monitor's native resolution:
//!
//! 1. **Stretch** — fill the destination, distorting aspect ratio.
//!    Used for the fullscreen screensaver presentation path.
//! 2. **Letterbox** — preserve aspect ratio with black bars.
//!    Used for preview windows and any path that respects the saver's
//!    intended aspect.
//!
//! Both paths use the [`cpu`] module's nearest-neighbor / bilinear
//! samplers. A future GPU backend (wgpu/Vulkan) could implement the same
//! [`FrameUpscaler`] trait without changing call sites, but the work is
//! not currently planned.

mod cpu;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Nearest,
    Linear,
}

impl FilterMode {
    /// Parse filter name (pure; unit-tested without env races).
    pub fn from_name(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "nearest" | "point" => Self::Nearest,
            _ => Self::Linear,
        }
    }

    pub fn from_env() -> Self {
        match idle_api::env_var_first(&["IDLE_GPU_FILTER"]).as_deref() {
            Some(s) => Self::from_name(s),
            None => Self::Linear,
        }
    }
}

/// Whether GPU upscaling should be attempted.
///
/// **Always returns `false`.** This function exists only as a placeholder
/// for historical callers that branched on GPU availability. The crate
/// contains no GPU code; all upscaling is CPU-based (see [`cpu`]).
///
/// Simulation grid scale factor in `(0, 1]`. Lower values render chunkier effects
/// that are upscaled to the monitor resolution.
pub fn render_scale() -> f32 {
    resolve_render_scale(None)
}

/// Clamp to `[lo, hi]`, rejecting non-finite input.
///
/// `f32::clamp` propagates NaN: both comparisons are false, so `NaN.clamp(a, b)`
/// returns NaN rather than a bound. `"nan".parse::<f32>()` is `Ok(NaN)`, and
/// `NaN as u32` is 0 — so an `IDLE_RENDER_SCALE=nan` in the systemd user
/// environment reached `f32` as NaN, survived `.clamp`, and produced a
/// zero-sized source geometry. The D-Bus path already rejects NaN at
/// `controller/commands.rs:115`; the env path did not.
fn clamp_finite(value: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(lo, hi)
    } else {
        fallback
    }
}

/// Effective simulation grid scale: env `IDLE_RENDER_SCALE`, then config.
pub fn resolve_render_scale(configured: Option<f32>) -> f32 {
    if let Some(scale) =
        idle_api::env_var_first(&["IDLE_RENDER_SCALE"]).and_then(|v| v.parse::<f32>().ok())
    {
        return clamp_finite(scale, 0.25, 1.0, 0.5);
    }
    if let Some(scale) = configured {
        return clamp_finite(scale, 0.25, 1.0, 0.5);
    }
    0.5
}

/// Presentation frame-rate cap. `0` means match the detected monitor refresh rate.
pub fn max_fps() -> u32 {
    idle_api::env_var_first(&["IDLE_MAX_FPS"])
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Physics / simulation tick rate (Hz). Independent of monitor refresh.
pub fn simulation_tick_hz() -> f32 {
    idle_api::env_var_first(&["IDLE_TICK_HZ"])
        .and_then(|value| value.parse::<f32>().ok())
        .map_or(60.0, |hz| clamp_finite(hz, 15.0, 240.0, 60.0))
}

pub fn target_fps(detected_refresh_hz: u32) -> f32 {
    let detected = detected_refresh_hz.max(60);
    let cap = max_fps();
    if cap == 0 {
        detected as f32
    } else {
        detected.min(cap) as f32
    }
}

pub use idle_api::GpuSpotlight;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuCell {
    pub ch: u32,
    pub fg: [u8; 4],
    pub bg: [u8; 4],
    pub bold: u32,
}

pub struct FrameUpscaler {
    filter: FilterMode,
    stretch_cache: cpu::StretchCache,
}

impl FrameUpscaler {
    pub fn new(filter: FilterMode) -> Self {
        Self {
            filter,
            stretch_cache: cpu::StretchCache::new(),
        }
    }

    pub fn using_gpu(&self) -> bool {
        false
    }

    pub fn adapter_name(&self) -> Option<&str> {
        None
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upscale_letterbox_into(
        &mut self,
        src: &[u8],
        src_w: u32,
        src_h: u32,
        dst_w: u32,
        dst_h: u32,
        out: &mut Vec<u8>,
    ) {
        let needed = (dst_w as usize)
            .checked_mul(dst_h as usize)
            .and_then(|p| p.checked_mul(4))
            .unwrap_or(0);
        out.resize(needed, 0);
        cpu::upscale_letterbox_into(out, src, src_w, src_h, dst_w, dst_h, self.filter);
    }

    /// Stretch source to fill the destination (fullscreen presentation path).
    #[allow(clippy::too_many_arguments)]
    pub fn upscale_stretch_into(
        &mut self,
        src: &[u8],
        src_w: u32,
        src_h: u32,
        dst_w: u32,
        dst_h: u32,
        out: &mut Vec<u8>,
    ) {
        let needed = (dst_w as usize)
            .checked_mul(dst_h as usize)
            .and_then(|p| p.checked_mul(4))
            .unwrap_or(0);
        out.resize(needed, 0);
        cpu::upscale_stretch_into(
            out,
            src,
            src_w,
            src_h,
            dst_w,
            dst_h,
            &mut self.stretch_cache,
        );
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(test)]
mod nan_guard_tests {
    use super::*;

    /// Env is process-global; these tests set and remove it.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn resolve_render_scale_rejects_nan_from_env() {
        let _u = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("IDLE_RENDER_SCALE", "nan") };
        let s = resolve_render_scale(None);
        unsafe { std::env::remove_var("IDLE_RENDER_SCALE") };
        assert!(
            s.is_finite() && (0.25..=1.0).contains(&s),
            "IDLE_RENDER_SCALE=nan must not produce {s}"
        );
        assert_eq!(s, 0.5, "non-finite env falls back to the default");
    }

    #[test]
    fn resolve_render_scale_rejects_nan_from_config() {
        let s = resolve_render_scale(Some(f32::NAN));
        assert!(s.is_finite(), "configured NaN must not survive the clamp");
        assert_eq!(s, 0.5);
    }

    #[test]
    fn simulation_tick_hz_rejects_nan() {
        let s = clamp_finite(f32::NAN, 15.0, 240.0, 60.0);
        assert_eq!(s, 60.0);
    }

    #[test]
    fn clamp_finite_keeps_ordinary_values() {
        assert_eq!(clamp_finite(0.5, 0.25, 1.0, 0.5), 0.5);
        assert_eq!(clamp_finite(0.1, 0.25, 1.0, 0.5), 0.25);
        assert_eq!(clamp_finite(9.0, 0.25, 1.0, 0.5), 1.0);
    }
}
