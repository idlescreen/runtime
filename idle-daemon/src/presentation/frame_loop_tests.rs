// SPDX-License-Identifier: MIT
// perf: T3 · metric: test-only page, not compiled into the shipped binary · check: test

//! Unit tests for `frame_loop`. Kept in a sibling file so the main
//! `frame_loop.rs` stays under the repo's 256-line cap (CI gate at
//! `.github/workflows/ci.yml::file length cap`).

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use idle_api::{OutputLayout, OverlaySurface};

use super::frame_loop::run_frame_loop;
use crate::presentation::PresentationOptions;

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
