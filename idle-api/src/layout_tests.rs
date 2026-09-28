// perf: T3 · metric: test-only page, not compiled into the shipped binary · check: test
// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Makes `layout.rs`'s cost claim enforceable.
//!
//! `is_span_layout` decides which renderer the daemon picks and
//! `span_reach_scale` is consulted while composing, so both sit on the
//! presentation path. Neither allocates. Neither is allowed to start.

use perf_test_support::{assert_no_alloc, count_allocs};

use super::{is_span_layout, span_reach_scale};

#[test]
fn span_layout_predicates_do_not_allocate() {
    for (cols, rows) in [(80usize, 24usize), (160, 48), (210, 57), (1, 1)] {
        let _ = assert_no_alloc("is_span_layout", || is_span_layout(cols, rows));
        let _ = assert_no_alloc("span_reach_scale", || span_reach_scale(cols, rows));
    }
}

/// The counter must be able to see an allocation, or the test above is
/// decorative.
#[test]
fn the_counter_does_see_allocations() {
    let (_, n) = count_allocs(|| {
        let owned: Vec<u8> = vec![0; 8];
        std::hint::black_box(owned.len())
    });
    assert!(n >= 1, "the counting allocator reported {n}");
}
