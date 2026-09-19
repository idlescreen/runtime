// SPDX-License-Identifier: MIT

#![allow(clippy::too_many_arguments)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::ipc_session::IpcPluginSession;
use idle_api::{OutputLayout, OverlaySurface};

use super::render::present_frame;
use crate::presentation::PresentationOptions;

pub struct ActiveSession {
    pub output_id: u32,
    pub session: IpcPluginSession,
    pub cols: usize,
    pub rows: usize,
}

/// Per-frame loop locals: inputs + state mutated across iterations.
pub struct FrameLoopState<'a> {
    pub presenter: &'a dyn OverlaySurface,
    pub stop: &'a AtomicBool,
    pub sessions: &'a mut [ActiveSession],
    pub layouts: &'a [OutputLayout],
    pub primary: OutputLayout,
    pub independent_rendering: bool,
    pub options: PresentationOptions,
    pub present_fps: f32,
    pub tick_hz: f32,
    pub frame_duration: Duration,
    pub last_frame: Instant,
    pub frame_start: Instant,
    pub frame_counter: u64,
    pub fps_report: Instant,
    pub achieved_fps: f32,
    pub use_hw_scaling: bool,
    pub session_start: Instant,
}

pub fn run_frame_loop(
    presenter: &dyn OverlaySurface,
    stop: &AtomicBool,
    sessions: &mut [ActiveSession],
    layouts: &[OutputLayout],
    primary: OutputLayout,
    independent_rendering: bool,
    options: PresentationOptions,
    present_fps: f32,
    tick_hz: f32,
    frame_duration: Duration,
    last_frame: &mut Instant,
    frame_counter: &mut u64,
    fps_report: &mut Instant,
    achieved_fps: &mut f32,
) -> Result<(), String> {
    if sessions.is_empty() {
        return Err("No active sessions provided to frame loop".into());
    }

    // COSMIC (and some other compositors) have disconnected the Wayland client
    // when wp_viewporter set_destination is used during screensaver preview.
    // That used to kill the whole daemon via check_runtime_alive. Opt-in only.
    let use_hw_scaling = super::hw_scaling::should_use_hw_viewport(
        super::hw_scaling::hw_viewport_env_force(),
        presenter.supports_scaling(),
        sessions[0].session.using_gpu_upscale(),
    );
    for s in sessions.iter_mut() {
        s.session.set_hardware_scaling(use_hw_scaling);
    }
    if use_hw_scaling {
        idle_log::info!(
            "wayland-present: hardware scaling enabled via wp_viewporter (IDLE_HW_VIEWPORT)"
        );
    } else if presenter.supports_scaling() {
        idle_log::debug!(
            "wayland-present: wp_viewporter available but disabled (set IDLE_HW_VIEWPORT=1 to enable)"
        );
    }

    let mut state = FrameLoopState {
        presenter,
        stop,
        sessions,
        layouts,
        primary,
        independent_rendering,
        options,
        present_fps,
        tick_hz,
        frame_duration,
        last_frame: *last_frame,
        frame_start: *last_frame,
        frame_counter: *frame_counter,
        fps_report: *fps_report,
        achieved_fps: *achieved_fps,
        use_hw_scaling,
        session_start: Instant::now(),
    };

    while !state.stop.load(Ordering::Relaxed) && state.presenter.is_visible() {
        state.frame_counter += 1;
        let frame_index = state.frame_counter;
        prepare_frame(&mut state)?;
        present_frame(&mut state);
        update_fps_counter(&mut state, frame_index);
    }

    *last_frame = state.last_frame;
    *frame_counter = state.frame_counter;
    *fps_report = state.fps_report;
    *achieved_fps = state.achieved_fps;
    Ok(())
}

fn prepare_frame(state: &mut FrameLoopState) -> Result<(), String> {
    let frame_start = Instant::now();
    let frame_dt = frame_start.saturating_duration_since(state.last_frame);
    state.last_frame = frame_start;
    state.frame_start = frame_start;
    for s in state.sessions.iter_mut() {
        if !s.session.is_plugin_alive() {
            s.session.recover(s.cols, s.rows)?;
            s.session.set_simulation_rate(state.tick_hz);
        }
        s.session.tick(frame_dt);
    }
    Ok(())
}

fn update_fps_counter(state: &mut FrameLoopState, frame_index: u64) {
    let elapsed = state.frame_start.elapsed();
    if state.fps_report.elapsed() >= Duration::from_secs(1) {
        state.achieved_fps = frame_index as f32 / state.fps_report.elapsed().as_secs_f32();
        if frame_index >= state.present_fps as u64
            || state.fps_report.elapsed() >= Duration::from_secs(5)
        {
            idle_log::info!(
                "achieved {:.1} FPS (target {:.0}, tick {:.0})",
                state.achieved_fps,
                state.present_fps,
                state.tick_hz
            );
            state.fps_report = Instant::now();
            state.frame_counter = 0;
        }
    }

    if elapsed < state.frame_duration {
        // saturating_sub: frame overruns must not panic the daemon
        // (L1 lifecycle bug — under load, `frame_duration - elapsed`
        // would panic on negative).
        thread::sleep(state.frame_duration.saturating_sub(elapsed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::PresentationOptions;
    use idle_api::OverlaySurface;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    /// Stub surface for tests — always reports dead so we can exercise
    /// the empty-sessions early-return without a Wayland environment.
    struct TestStub;
    impl OverlaySurface for TestStub {
        fn is_available() -> bool {
            false
        }
        fn new() -> Option<Self> {
            Some(Self)
        }
        fn submit_frame(&self, _: idle_api::OutputId, _: std::sync::Arc<Vec<u8>>, _: u32, _: u32) {}
        fn is_alive(&self) -> bool {
            false
        }
        fn is_visible(&self) -> bool {
            false
        }
        fn show_blank(&self, _: idle_api::BlankAppearance) {}
        fn show_screensaver(&self) {}
        fn hide(&self) {}
        fn supports_scaling(&self) -> bool {
            false
        }
        fn output_layouts(&self) -> Vec<OutputLayout> {
            Vec::new()
        }
    }

    // Test negative selection: empty sessions slice securely returns error instead of panicking on [0]
    #[test]
    fn test_empty_sessions_returns_error() {
        let presenter: Box<dyn OverlaySurface> = Box::new(TestStub);

        let stop = AtomicBool::new(false);
        let mut sessions = vec![];
        let layouts = vec![];
        let primary = OutputLayout {
            id: 0,
            width: 800,
            height: 600,
            x: 0,
            y: 0,
            refresh_mhz: 60,
            scale: 1,
        };

        let mut last_frame = Instant::now();
        let mut frame_counter = 0;
        let mut fps_report = Instant::now();
        let mut achieved_fps = 0.0;

        let result = run_frame_loop(
            &*presenter,
            &stop,
            &mut sessions,
            &layouts,
            primary,
            false,
            PresentationOptions {
                show_fps_overlay: false,
                render_scale: None,
                launch_mode: idle_runner::launcher::LaunchMode::Daemon,
                saver_params: std::collections::BTreeMap::new(),
            },
            60.0,
            60.0,
            Duration::from_millis(16),
            &mut last_frame,
            &mut frame_counter,
            &mut fps_report,
            &mut achieved_fps,
        );

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "No active sessions provided to frame loop"
        );
    }
}
