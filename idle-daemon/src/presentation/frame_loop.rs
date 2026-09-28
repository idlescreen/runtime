// SPDX-License-Identifier: MIT
// perf: T3 · metric: iterative; cost scales with its input, not with a fixed bound · check: review

#![allow(clippy::too_many_arguments)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::ipc_session::IpcPluginSession;
use idle_api::{OutputLayout, OverlaySurface};

use super::present_frame::present_frame;
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
            // A saver that keeps missing the IPC deadline must not be respawned
            // every frame: each respawn re-runs Landlock, cgroup attach,
            // renderer init and a fresh SHM mapping, and the loop fires
            // hardest on the slowest frames. Give up after the budget and let
            // the daemon's fault/cooldown path own the retry.
            if !s.session.should_recover(s.cols, s.rows) {
                let timeouts = s.session.consecutive_timeouts;
                s.session.mark_exhausted(s.cols, s.rows);
                return Err(format!(
                    "saver {} timed out {timeouts}× (budget {}) at {}x{}; \
                     refusing to respawn — stopping presentation",
                    s.session.saver_name,
                    IpcPluginSession::max_consecutive_timeouts(),
                    s.cols,
                    s.rows
                ));
            }
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
        let remaining = state.frame_duration.saturating_sub(elapsed);
        // Tier-2 perf change (round 1): poll the `stop` flag during the
        // sleep so shutdown interrupts within ~2 ms instead of waiting
        // out the full frame duration.
        //
        // Tier-2 step 2.5 (round 2): wait on the presenter's frame
        // signal instead of slicing. The presenter notifies after a
        // successful surface commit (and, when the `wl_callback::done`
        // dispatch hook is wired in `wayland-present`, on actual
        // vsync). Avoids the systematic 2 ms polling wakeup that was
        // stealing ~30 wakeups/sec from the daemon even when no frames
        // were being produced.
        //
        // If the presenter does not expose a frame signal (stub or
        // platform impl without Wayland), fall back to the slice-poll.
        if let Some(signal) = state.presenter.frame_signal() {
            // Block up to `remaining`. If the signal fires earlier we
            // wake, re-check the loop, and either draw the next frame
            // immediately or wait again for the next slice.
            let _ = signal.wait_for(state.stop, remaining);
        } else {
            sleep_interruptible(remaining, state.stop);
        }
    }
}

/// Sleep up to `remaining`, polling `stop` every `SLICE` so a Ctrl-C
/// or presenter-detach interrupts within a slice boundary.
///
/// Used as a fallback when the presenter has no frame signal (stub
/// implementations, platform impls without vsync plumbing). Returns
/// as soon as either `stop` flips or `remaining` elapses.
fn sleep_interruptible(remaining: Duration, stop: &AtomicBool) {
    const SLICE: Duration = Duration::from_millis(2);
    let deadline = Instant::now() + remaining;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let slice = (deadline - now).min(SLICE);
        thread::sleep(slice);
    }
}
