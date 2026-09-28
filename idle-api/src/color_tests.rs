// perf: T3 · metric: test-only page, not compiled into the shipped binary · check: test
// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Makes `color.rs`'s cost claim enforceable.
//!
//! HSL/RGB conversion runs for every cell of every frame a saver draws,
//! so an allocation dropped into it is a per-frame cost paid for the
//! life of the process, on battery. The page promises none. This makes
//! that promise a build failure instead of a comment.
//!
//! It lives in its own page because `color.rs` was already within a few
//! lines of the 256 ceiling and the assertions did not fit under it.

use perf_test_support::{assert_no_alloc, count_allocs};

use super::{hsl_to_rgb, lerp, percentage, rgb_to_hsl};

#[test]
fn colour_conversion_does_not_allocate() {
    for (h, s, l) in [
        (0.0f32, 0.0f32, 0.0f32),
        (210.0, 0.5, 0.5),
        (359.9, 1.0, 1.0),
    ] {
        let _ = assert_no_alloc("hsl_to_rgb", || hsl_to_rgb(h, s, l));
    }
    for (r, g, b) in [(0u8, 0u8, 0u8), (248, 248, 242), (255, 128, 0)] {
        let _ = assert_no_alloc("rgb_to_hsl", || rgb_to_hsl(r, g, b));
    }
    let _ = assert_no_alloc("percentage", || percentage(3, 7));
    let _ = assert_no_alloc("lerp", || lerp(0.0, 10.0, 0.25));
}

/// The counter must be able to see an allocation, or the test above is
/// decorative — a silently dead allocator would make it always pass.
#[test]
fn the_counter_does_see_allocations() {
    let (_, n) = count_allocs(|| {
        let owned: Vec<u8> = vec![0; 8];
        std::hint::black_box(owned.len())
    });
    assert!(n >= 1, "the counting allocator reported {n}");
}
