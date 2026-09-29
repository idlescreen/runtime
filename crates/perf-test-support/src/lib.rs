// SPDX-License-Identifier: MIT

//! Turns a `metric:` claim into something a test can fail on.
//!
//! A page labelled `no allocation on the steady path` is making a
//! promise. This crate is how the promise gets checked: it installs a
//! counting [`GlobalAllocator`] and exposes [`count_allocs`] /
//! [`assert_no_alloc`], so a test can say "this call allocates nothing"
//! and the build goes red the moment someone helpfully adds a `Vec`.
//!
//! # Why a thread-local
//!
//! `cargo test` runs tests in parallel threads. A single global counter
//! would make every assertion in the binary race with every other one.
//! The counter is thread-local, so a test observes only the allocations
//! made by its own thread between [`count_allocs`] and its return.
//!
//! # Scope
//!
//! This measures the calling thread. Work handed to another thread, or
//! performed by a runtime that already holds a warm buffer, is not
//! counted — assertions are written against one call on one thread, and
//! the test name says which call.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    /// `None` when not counting. `Some(n)` while a measurement is open.
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

/// Counts every allocation made by the current thread while a
/// measurement is open, and delegates to the system allocator.
pub struct CountingAlloc;

/// `#[global_allocator]` applies wherever this crate is in the graph.
/// Because it is only ever a dev-dependency, that is exactly the test
/// binaries that link it and never a shipped binary.
#[global_allocator]
static COUNTING_ALLOC: CountingAlloc = CountingAlloc;

// SAFETY: every method forwards to `System` unchanged. The only added
// behaviour is a thread-local counter, which is touched only on the
// calling thread and is therefore not shared mutable state.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump();
        // SAFETY: `layout` is forwarded verbatim to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` came from `System::alloc` above.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A realloc is an allocation that may move. Count it: growing a
        // buffer on the steady path is exactly the regression these
        // assertions exist to catch.
        bump();
        // SAFETY: forwarded verbatim.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        bump();
        // SAFETY: forwarded verbatim.
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[inline]
fn bump() {
    // `try_with` keeps the allocator usable during thread teardown, when
    // a thread_local may already be destroyed.
    let _ = COUNTING.try_with(|c| {
        if c.get() {
            let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
        }
    });
}

/// Run `f`, returning its value and the number of allocations it made on
/// this thread.
///
/// Warm-up matters: the first call into a code path often allocates
/// because a lazily-initialised buffer is still empty. Call the
/// function once before measuring, exactly as the benchmarks do.
pub fn count_allocs<T, F: FnOnce() -> T>(f: F) -> (T, u64) {
    ALLOCS.with(|n| n.set(0));
    COUNTING.with(|c| c.set(true));
    let out = f();
    COUNTING.with(|c| c.set(false));
    (out, ALLOCS.with(|n| n.get()))
}

/// Assert that `f` allocates nothing on this thread, and return its
/// value.
///
/// The panic message names the page under test and the call, because a
/// bare "assertion failed" in a 700-test binary tells you nothing.
#[track_caller]
pub fn assert_no_alloc<T, F: FnOnce() -> T>(what: &str, f: F) -> T {
    let (out, n) = count_allocs(f);
    assert_eq!(
        n, 0,
        "{what} allocated {n} time(s); its label promises none"
    );
    out
}

/// Assert that `f` allocates no more than `budget` times on this thread,
/// and return its value. Use where a page genuinely allocates a fixed
/// number of times, and the point is that the number does not grow.
#[track_caller]
pub fn assert_alloc_at_most<T, F: FnOnce() -> T>(what: &str, budget: u64, f: F) -> T {
    let (out, n) = count_allocs(f);
    assert!(
        n <= budget,
        "{what} allocated {n} time(s), over its budget of {budget}"
    );
    out
}

#[cfg(test)]
mod tests;
