// SPDX-License-Identifier: MIT

//! Present/simulation frame pacing for the plugin presentation loop.

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use idle_api::{OutputLayout, OverlaySurface};

use super::frame_loop::{ActiveSession, run_frame_loop};
use crate::presentation::PresentationOptions;
use crate::presentation::refresh::presentation_refresh_hz;
use crate::presentation::session::IpcPluginSession;
use idle_upscaler::{simulation_tick_hz, target_fps};

/// Clamp present FPS so `Duration::from_secs_f32(1.0 / fps)` never sees 0/NaN/∞.
pub(crate) fn clamp_present_fps(present_fps: f32) -> f32 {
    if present_fps.is_finite() && present_fps > 0.0 {
        present_fps.clamp(1.0, 480.0)
    } else {
        60.0
    }
}

/// Clamp simulation tick Hz (matches upscaler floor/ceiling + non-finite guard).
pub(super) fn clamp_tick_hz(tick_hz: f32) -> f32 {
    if tick_hz.is_finite() && tick_hz > 0.0 {
        tick_hz.clamp(15.0, 240.0)
    } else {
        60.0
    }
}

/// Adaptive refresh rate pacing stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptivePacingStage {
    /// Initial high-responsiveness or preview phase (60 or 144 Hz).
    Interactive,
    /// Standard presentation phase (60 Hz).
    Active,
    /// Extended idle energy-saving phase (24 or 30 Hz).
    DeepAmbient,
    /// Battery conservation phase (capped at 30 Hz).
    Battery,
}

/// Compute throttled (or nominal) presentation FPS and simulation tick Hz.
pub fn apply_power_throttling(nominal_fps: f32, nominal_tick: f32, on_battery: bool) -> (f32, f32) {
    if on_battery {
        (nominal_fps.min(30.0), nominal_tick.clamp(15.0, 30.0))
    } else {
        (nominal_fps, nominal_tick)
    }
}

/// Dynamic adaptive pacing: Interactive (60/144 Hz) -> Active (60 Hz) -> Deep Ambient (24/30 Hz) -> Battery (30 Hz).
pub fn resolve_adaptive_pacing(
    nominal_refresh_hz: f32,
    elapsed: Duration,
    on_battery: bool,
) -> (f32, f32, AdaptivePacingStage) {
    if on_battery {
        return (30.0, 30.0, AdaptivePacingStage::Battery);
    }
    if elapsed < Duration::from_secs(5) {
        let fps = nominal_refresh_hz.clamp(60.0, 144.0);
        (fps, 60.0, AdaptivePacingStage::Interactive)
    } else if elapsed < Duration::from_secs(45) {
        let fps = 60.0f32.min(nominal_refresh_hz.max(30.0));
        (fps, 60.0, AdaptivePacingStage::Active)
    } else {
        let fps = if (nominal_refresh_hz % 24.0).abs() < 1.0 {
            24.0
        } else {
            30.0
        };
        (fps, 30.0, AdaptivePacingStage::DeepAmbient)
    }
}

/// Check if the adaptive pacing stage has changed based on session elapsed time or power source.
pub(crate) fn check_adaptive_transition(
    nominal_fps: f32,
    session_start: Instant,
    on_battery: bool,
    current_fps: f32,
    current_tick: f32,
) -> Option<(f32, f32, AdaptivePacingStage)> {
    let elapsed = session_start.elapsed();
    let (new_fps, new_tick, stage) = resolve_adaptive_pacing(nominal_fps, elapsed, on_battery);
    if (new_fps - current_fps).abs() > 0.01 || (new_tick - current_tick).abs() > 0.01 {
        Some((new_fps, new_tick, stage))
    } else {
        None
    }
}

pub(crate) struct FramePacing {
    nominal_fps: f32,
    nominal_tick: f32,
    on_battery: bool,
    present_fps: f32,
    tick_hz: f32,
    frame_duration: Duration,
    last_frame: Instant,
    frame_counter: u64,
    fps_report: Instant,
    achieved_fps: f32,
}

impl FramePacing {
    pub(crate) fn compute(
        layouts: &[OutputLayout],
        primary: OutputLayout,
        sessions: &mut [ActiveSession],
    ) -> Self {
        let present_refresh = presentation_refresh_hz(layouts, primary);
        let mut nominal_fps = target_fps(present_refresh);
        let mut nominal_tick = simulation_tick_hz();

        if present_refresh > 0 {
            nominal_fps = nominal_fps.min(present_refresh as f32);
            nominal_tick = nominal_tick.min(present_refresh as f32);
        }

        let nominal_fps = clamp_present_fps(nominal_fps);
        let nominal_tick = clamp_tick_hz(nominal_tick);

        let on_battery = crate::daemon::battery::is_on_battery();
        let (present_fps, tick_hz, _stage) =
            resolve_adaptive_pacing(nominal_fps, Duration::ZERO, on_battery);
        if on_battery {
            idle_log::info!(
                "Battery power detected: capping physics simulation and rendering frame rate targets to 30 FPS/Hz"
            );
        }

        let frame_duration = Duration::from_secs_f32(1.0 / present_fps);
        for s in sessions {
            s.set_simulation_rate(tick_hz);
        }
        Self {
            nominal_fps,
            nominal_tick,
            on_battery,
            present_fps,
            tick_hz,
            frame_duration,
            last_frame: Instant::now(),
            frame_counter: 0,
            fps_report: Instant::now(),
            achieved_fps: 0.0,
        }
    }

    pub(crate) fn present_fps(&self) -> f32 {
        self.present_fps
    }

    pub(crate) fn tick_hz(&self) -> f32 {
        self.tick_hz
    }

    pub(crate) fn run_loop(
        mut self,
        presenter: &dyn OverlaySurface,
        stop: &AtomicBool,
        sessions: &mut [ActiveSession],
        layouts: &[OutputLayout],
        primary: OutputLayout,
        independent_rendering: bool,
        options: PresentationOptions,
    ) -> Result<(), String> {
        run_frame_loop(
            presenter,
            stop,
            sessions,
            layouts,
            primary,
            independent_rendering,
            options,
            self.nominal_fps,
            self.nominal_tick,
            self.on_battery,
            self.present_fps,
            self.tick_hz,
            self.frame_duration,
            &mut self.last_frame,
            &mut self.frame_counter,
            &mut self.fps_report,
            &mut self.achieved_fps,
        )
    }
}

pub(crate) fn log_run_startup(
    saver_name: &str,
    layouts: &[OutputLayout],
    pacing: &FramePacing,
    session: &IpcPluginSession,
) {
    idle_log::info!(
        "running plugin '{}' on {} monitor(s) at {:.0} FPS / {:.0} tick (render scale {:.0}%, GPU: {})",
        saver_name,
        layouts.len(),
        pacing.present_fps(),
        pacing.tick_hz(),
        session.render_scale() * 100.0,
        if session.using_gpu_upscale() {
            "yes"
        } else {
            "no"
        }
    );
}

#[cfg(test)]
#[path = "frame_pacing_tests.rs"]
mod tests;
