// SPDX-License-Identifier: MIT

//! Present/simulation frame pacing for the plugin presentation loop.

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use idle_api::{OutputLayout, OverlaySurface};

use super::frame_loop::{ActiveSession, run_frame_loop};
use super::ipc_session::IpcPluginSession;
use super::refresh::presentation_refresh_hz;
use crate::presentation::PresentationOptions;
use idle_upscaler::{simulation_tick_hz, target_fps};

/// Clamp present FPS so `Duration::from_secs_f32(1.0 / fps)` never sees 0/NaN/∞.
pub(super) fn clamp_present_fps(present_fps: f32) -> f32 {
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

pub(super) struct FramePacing {
    present_fps: f32,
    tick_hz: f32,
    frame_duration: Duration,
    last_frame: Instant,
    frame_counter: u64,
    fps_report: Instant,
    achieved_fps: f32,
}

impl FramePacing {
    pub(super) fn compute(
        layouts: &[OutputLayout],
        primary: OutputLayout,
        sessions: &mut [ActiveSession],
    ) -> Self {
        let present_refresh = presentation_refresh_hz(layouts, primary);
        let mut present_fps = target_fps(present_refresh);
        let mut tick_hz = simulation_tick_hz();

        let sys = idle_runner::toolkit::sys_info::get_system_info();
        if sys.power_status.contains("Battery") {
            present_fps = present_fps.min(30.0);
            tick_hz = tick_hz.min(30.0);
            idle_log::info!(
                "Battery power detected: capping physics simulation and rendering frame rate targets to 30 FPS/Hz"
            );
        }

        // target_fps / simulation_tick_hz already floor at ≥15; clamp again so a
        // future regression cannot pass 0/NaN into Duration::from_secs_f32.
        let present_fps = clamp_present_fps(present_fps);
        let tick_hz = clamp_tick_hz(tick_hz);
        let frame_duration = Duration::from_secs_f32(1.0 / present_fps);
        for s in sessions {
            s.session.set_simulation_rate(tick_hz);
        }
        Self {
            present_fps,
            tick_hz,
            frame_duration,
            last_frame: Instant::now(),
            frame_counter: 0,
            fps_report: Instant::now(),
            achieved_fps: 0.0,
        }
    }

    pub(super) fn present_fps(&self) -> f32 {
        self.present_fps
    }

    pub(super) fn tick_hz(&self) -> f32 {
        self.tick_hz
    }

    pub(super) fn run_loop(
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

pub(super) fn log_run_startup(
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
