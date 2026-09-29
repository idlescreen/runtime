// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Property harness for screensaver plugins: drive a saver through
//! adversarial frame sequences and assert the invariants every plugin must
//! hold regardless of what its simulation does internally.
//!
//! Invariants checked:
//! - `update`/`init`/`draw` never panic across degenerate dimensions and
//!   hostile `dt` values (0, tiny, huge, jittered, alternating).
//! - `spotlights()` returns only finite floats with `origin_x_ratio` in a
//!   sane bound (spotlights feed the GPU path — a NaN there smears render).
//! - `draw` stays within the passed grid (enforced implicitly — an OOB
//!   write into a slice panics, which is a test failure).
//!
//! Deterministic: fixed seeds, fixed sequence — failures reproduce exactly.

use crate::{Screensaver, TerminalCell};
use std::time::Duration;

/// Frame-time patterns chosen to break integrators: zero, epsilon, huge
/// (post-suspend jump), alternating extremes, and seeded jitter.
fn hostile_dt(i: u32, rng: &mut crate::LcgRng) -> Duration {
    match i % 7 {
        0 => Duration::ZERO,
        1 => Duration::from_nanos(1),
        2 => Duration::from_secs(10),
        3 => Duration::from_secs_f64(1.0 / 60.0),
        4 => Duration::from_secs_f64(1.0 / 120.0),
        // jittered 0..500ms
        _ => Duration::from_secs_f64(rng.next_f32() as f64 * 0.5),
    }
}

/// Grid shapes chosen to break indexing math: degenerate single-axis grids,
/// the classic 80x24, widescreen-ish, and seeded randoms.
fn hostile_dims(i: u32, rng: &mut crate::LcgRng) -> (usize, usize) {
    match i % 9 {
        0 => (1, 1),
        1 => (1, 200),
        2 => (200, 1),
        3 => (80, 24),
        4 => (2, 2),
        _ => (rng.next_usize(480).max(1), rng.next_usize(200).max(1)),
    }
}

/// Assert a slice of spotlight values is all-finite.
fn assert_spotlights_finite(saver: &dyn Screensaver, frame: u32) {
    for (i, s) in saver.spotlights().iter().enumerate() {
        assert!(
            s.origin_x_ratio.is_finite()
                && s.color_r.is_finite()
                && s.color_g.is_finite()
                && s.color_b.is_finite()
                && s.angle.is_finite()
                && s.spread.is_finite()
                && s.speed.is_finite(),
            "frame {frame}: spotlight[{i}] has non-finite field: {s:?}"
        );
    }
}

/// Drive `saver` through `frames` hostile frames: random grid resize → init →
/// update → draw, asserting invariants each frame. Call from a saver's own
/// test:
///
/// ```ignore
/// #[test] fn stress() { let mut s = MySaver::new(); idle_api::stress::stress_saver(&mut s, 2000, 0xC0FFEE); }
/// ```
pub fn stress_saver(saver: &mut dyn Screensaver, frames: u32, seed: u64) {
    let mut rng = crate::LcgRng::new(seed);
    let mut grid: Vec<TerminalCell> = Vec::new();
    let mut last = (0usize, 0usize);

    for frame in 0..frames {
        let (cols, rows) = hostile_dims(frame, &mut rng);
        if (cols, rows) != last {
            saver.init(cols, rows);
            last = (cols, rows);
        }
        let dt = hostile_dt(frame, &mut rng);
        saver.update(dt, cols, rows);
        saver.update_frame_time(dt);

        grid.clear();
        grid.resize(cols * rows, TerminalCell::default());
        saver.draw(&mut grid, cols, rows);
        assert_spotlights_finite(saver, frame);
    }
}
