//! Fullscreen animation loop helpers split out from `idle_runner.rs`.
//!
//! `setup_terminal` installs signal handlers and enters raw mode; `teardown_terminal`
//! flushes pending input before the `RawTerminalGuard` drops; `drive_plugin_loop`
//! runs the actual frame loop. RAII for the terminal is provided by
//! `TerminalContext`, which owns the `RawTerminalGuard` acquired during setup.
//!
//! Note: Classic xscreensaver embedding support (XSCREENSAVER_WINDOW + xterm -into)
//! has been removed. The user runs via idle-daemon fullscreen xterm or crateria
//! previews, which do not require X11 embedding. Raw terminal + ANSI works on
//! Wayland (via xterm under XWayland or native terminals).

use crate::core::TerminalCell;
use crate::core::screensaver::Screensaver;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::{platform_helpers, renderer, terminal_guard};

/// RAII bundle for everything `setup_terminal` acquires. Dropping this restores
/// the terminal to its cooked mode (via `RawTerminalGuard::Drop`).
pub(super) struct TerminalContext {
    _raw_mode: terminal_guard::RawTerminalGuard,
}

/// Install signal handlers and enter raw terminal mode.
pub(super) fn setup_terminal() -> Result<TerminalContext, Box<dyn std::error::Error>> {
    #[cfg(not(target_os = "windows"))]
    unsafe {
        libc::signal(
            libc::SIGINT,
            super::handle_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            super::handle_signal as *const () as libc::sighandler_t,
        );
    }

    let raw_mode = match terminal_guard::RawTerminalGuard::enable() {
        Some(g) => g,
        None => {
            idle_log::error!("screensaver: could not enter raw mode; aborting.");
            return Err("could not enter raw mode".into());
        }
    };
    Ok(TerminalContext {
        _raw_mode: raw_mode,
    })
}

/// Flush any pending input before the raw-mode guard drops. Order matters:
/// tcflush must run while the terminal is still in raw mode.
pub(super) fn teardown_terminal(_ctx: TerminalContext) {
    #[cfg(not(target_os = "windows"))]
    unsafe {
        libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
    }
}

/// Returns `false` if the loop should exit (shutdown, late keypress, or mouse
/// activity). Also reapplies resize changes when the terminal dimensions shift.
#[allow(clippy::too_many_arguments)]
fn check_input_and_resize(
    saver: &mut dyn Screensaver,
    cols: &mut usize,
    rows: &mut usize,
    grid: &mut Vec<TerminalCell>,
    r: &mut renderer::Renderer,
    start_time: std::time::Instant,
    initial_mouse_pos: &mut Option<(i32, i32)>,
) -> bool {
    if super::SHUTDOWN.load(Ordering::Relaxed) {
        return false;
    }

    let is_startup = start_time.elapsed() < Duration::from_millis(500);

    if platform_helpers::check_keypress() {
        if is_startup {
            #[cfg(not(target_os = "windows"))]
            unsafe {
                libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
            }
        } else {
            return false;
        }
    }

    // Prevent instant exit on startup due to initial mouse shake or clicks
    if !is_startup && platform_helpers::check_mouse_activity(initial_mouse_pos) {
        return false;
    }

    // Handle terminal resize dynamically
    let (new_cols, new_rows) = platform_helpers::get_terminal_size();
    if new_cols != *cols || new_rows != *rows {
        *cols = new_cols;
        *rows = new_rows;
        *grid = vec![TerminalCell::default(); *cols * *rows];
        saver.init(*cols, *rows);
        *r = renderer::Renderer::new(*cols, *rows);
    }

    true
}

/// Snap the frame budget to the closest standard refresh rate, then derive the
/// decoupled physics tick rate from it. No-op for the first 19 frames.
fn calibrate_fps(
    frame_time_sum: f32,
    frame_duration: &mut Duration,
    physics_hz: &mut f32,
    physics_duration: &mut Duration,
) {
    let avg_frame_time = frame_time_sum / 20.0;
    if avg_frame_time <= 0.001 {
        return;
    }
    let measured_fps = 1.0 / avg_frame_time;
    let mut snapped_fps = measured_fps;
    for &std_rate in &[30.0, 60.0, 75.0, 90.0, 120.0, 144.0, 240.0] {
        if (measured_fps - std_rate).abs() < 4.0 {
            snapped_fps = std_rate;
            break;
        }
    }
    *frame_duration = Duration::from_secs_f32(1.0 / snapped_fps);
    let k = ((120.0 / snapped_fps).ceil() as u32).max(1);
    *physics_hz = snapped_fps * k as f32;
    *physics_duration = Duration::from_secs_f32(1.0 / *physics_hz);
}

/// Drain the physics accumulator by stepping the saver at the target tick rate.
/// Clamps the accumulator to avoid a spiral-of-death after long stalls.
fn step_physics(
    saver: &mut dyn Screensaver,
    dt: Duration,
    cols: usize,
    rows: usize,
    physics_accumulator: &mut Duration,
    physics_duration: Duration,
) {
    *physics_accumulator += dt;
    if *physics_accumulator > Duration::from_millis(100) {
        *physics_accumulator = Duration::from_millis(100);
    }
    while *physics_accumulator >= physics_duration {
        saver.update(physics_duration, cols, rows);
        *physics_accumulator -= physics_duration;
    }
}

/// Run the animation loop until shutdown signal, keypress, or mouse activity.
pub(super) fn drive_plugin_loop(
    saver: &mut dyn Screensaver,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut cols, mut rows) = platform_helpers::get_terminal_size();
    saver.init(cols, rows);
    let mut r = renderer::Renderer::new(cols, rows);
    let mut grid = vec![TerminalCell::default(); cols * rows];
    let mut last_frame = std::time::Instant::now();
    let target_fps = platform_helpers::get_monitor_refresh_rate().max(10);
    let mut frame_duration = Duration::from_secs_f32(1.0 / target_fps as f32);
    let mut physics_hz = 120.0;
    let mut physics_duration = Duration::from_secs_f32(1.0 / physics_hz);
    let mut physics_accumulator = Duration::ZERO;
    let mut frame_count = 0;
    let mut frame_time_sum = 0.0;
    let mut calibrated = false;
    let mut initial_mouse_pos = None;
    let start_time = std::time::Instant::now();

    loop {
        if !check_input_and_resize(
            saver,
            &mut cols,
            &mut rows,
            &mut grid,
            &mut r,
            start_time,
            &mut initial_mouse_pos,
        ) {
            break;
        }
        let now = std::time::Instant::now();
        let dt = now.duration_since(last_frame);
        last_frame = now;
        saver.update_frame_time(dt);
        if !calibrated {
            frame_count += 1;
            frame_time_sum += dt.as_secs_f32();
            if frame_count == 20 {
                calibrate_fps(
                    frame_time_sum,
                    &mut frame_duration,
                    &mut physics_hz,
                    &mut physics_duration,
                );
                calibrated = true;
            }
        }
        step_physics(
            saver,
            dt,
            cols,
            rows,
            &mut physics_accumulator,
            physics_duration,
        );
        saver.draw(&mut grid, cols, rows);
        r.render_grid(&grid, cols, rows, saver.has_scanlines());
        let elapsed = now.elapsed();
        // saturate to ZERO on overrun; `Duration - Duration` panics on
        // underflow (L1 lifecycle bug — frame overruns crash the daemon).
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration.saturating_sub(elapsed));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// PROBE #1 finding regression: `frame_duration.saturating_sub(elapsed)`
    /// must NOT panic when `elapsed > frame_duration` (frame overrun under load).
    /// Reproduces the L1 lifecycle bug found in idle-runner + idle-daemon
    /// frame loop on 2026-08-10; both call sites now use saturating_sub.
    #[test]
    fn frame_overrun_does_not_panic() {
        use std::time::Duration;
        let frame_duration = Duration::from_millis(16);
        let elapsed = Duration::from_millis(20);
        // The old code did `frame_duration - elapsed` which panicked.
        // The fix uses saturating_sub which returns ZERO on underflow.
        let sleep_for = frame_duration.saturating_sub(elapsed);
        assert_eq!(sleep_for, Duration::ZERO);
    }
}
