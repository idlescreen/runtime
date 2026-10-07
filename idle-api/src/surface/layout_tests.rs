// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Makes `layout.rs`'s cost claim enforceable.
//!
//! `is_span_layout` decides which renderer the daemon picks and
//! `span_reach_scale` is consulted while composing, so both sit on the
//! presentation path. Neither allocates. Neither is allowed to start.

use perf_test_support::{assert_no_alloc, count_allocs};

use super::{is_span_layout, place_centered_logo, span_reach_scale};

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

#[test]
fn a_long_os_name_is_trimmed_to_the_grid_instead_of_overflowing() {
    // "Fedora Linux 44 (Server Edition)" is the exact string that produced a
    // 195-column block render and left savers with nothing to draw.
    let text = "Fedora Linux 44 (Server Edition)";
    let logo = place_centered_logo(80, 24, text, None).expect("logo");
    assert!(
        logo.width <= 80,
        "block was {} columns wide on an 80-column grid",
        logo.width
    );
}

#[test]
fn a_short_name_is_left_alone() {
    // Compare against the untrimmed render rather than a hand-computed
    // column count: fitting must be a no-op when the text already fits.
    let name = "Omarchy";
    let untouched = crate::logo_block::render_logo_block(name, None);
    let expected = untouched.iter().map(|l| l.chars().count()).max().unwrap();

    let logo = place_centered_logo(200, 50, name, None).expect("logo");
    assert_eq!(
        logo.width, expected,
        "a name that already fits must not be cut"
    );
}

#[test]
fn an_absurdly_narrow_grid_drops_the_logo_rather_than_panicking() {
    assert!(place_centered_logo(3, 24, "Anything", None).is_none());
}
