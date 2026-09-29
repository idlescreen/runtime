// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: MIT

//! Makes `caption.rs`'s cost claim enforceable instead of aspirational.
//!
//! That page keeps a `static Mutex<String>` and deliberately splits its
//! API in two: [`with_caption`] borrows so the present path never copies,
//! while [`caption_text`] clones and therefore allocates. The split is
//! easy to undo by accident — "simplifying" `with_caption` to return a
//! `String` compiles, looks nicer, and quietly reintroduces an allocation
//! on every frame.
//!
//! These tests pin the distinction with a counting allocator, so that
//! kind of refactor fails the build instead of costing a frame budget.

use perf_test_support::{assert_no_alloc, count_allocs};

use super::{caption_text, clear_caption, publish_caption, with_caption};

/// The present path must not allocate once the caption is warm.
///
/// Warm-up matters: the first `publish_caption` grows the shared
/// `String` from empty, and that growth is a real allocation. The claim
/// under test is about the *steady* path, so the buffer is filled first.
#[test]
fn with_caption_does_not_allocate_once_warm() {
    publish_caption("warm the buffer to its steady-state capacity");
    // A second, shorter publish clears and reuses the reserved capacity.
    publish_caption("steady state");

    let seen = assert_no_alloc("with_caption", || with_caption(|s| s.len()));
    assert!(seen > 0, "caption should be readable, got {seen} bytes");
}

/// The counter is real: the sibling API that clones must be seen to
/// allocate.
///
/// Without this, `with_caption_does_not_allocate_once_warm` would still
/// pass if the counting allocator were silently dead and every count
/// came back zero. Two assertions in opposite directions are what make
/// the first one mean something.
#[test]
fn caption_text_does_allocate_because_it_clones() {
    publish_caption("a caption long enough to force a fresh allocation");

    let (text, n) = count_allocs(caption_text);
    assert_eq!(text, "a caption long enough to force a fresh allocation");
    assert!(
        n >= 1,
        "caption_text clones into a new String, so it must allocate; \
         if this is zero the counting allocator is not working"
    );
}

/// Reading an empty caption still must not allocate.
#[test]
fn with_caption_on_empty_still_does_not_allocate() {
    clear_caption();
    let len = assert_no_alloc("with_caption (empty)", || with_caption(|s| s.len()));
    assert_eq!(len, 0);
}
