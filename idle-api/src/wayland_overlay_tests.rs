// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: Apache-2.0

//! Adversarial tests for the Linux overlay adapter.

use super::*;
use std::sync::Arc;

// WAYLAND_DISPLAY is process-global — tests mutating it must serialize.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn wayland_overlay_is_unavailable_without_session() {
    let _g = ENV_LOCK.lock().unwrap();
    let backup = std::env::var("WAYLAND_DISPLAY").ok();
    unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    assert!(!WaylandOverlay::is_available());
    if let Some(v) = backup {
        unsafe { std::env::set_var("WAYLAND_DISPLAY", v) };
    }
}

#[test]
fn wayland_overlay_lifecycle_smoke() {
    // With no real Wayland session, `new()` returns None. The test just
    // asserts the adapter never panics in either path.
    let _g = ENV_LOCK.lock().unwrap();
    let backup = std::env::var("WAYLAND_DISPLAY").ok();
    unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    let w = WaylandOverlay::new();
    assert!(
        w.is_none(),
        "without WAYLAND_DISPLAY the adapter must refuse"
    );
    if let Some(v) = backup {
        unsafe { std::env::set_var("WAYLAND_DISPLAY", v) };
    }
}

#[test]
fn trait_dispatch_via_dyn() {
    // Construct a fake surface via the StubOverlay and verify it can be
    // stored as Box<dyn OverlaySurface>. This exercises the trait seam
    // end-to-end without a real Wayland session.
    let surface: Box<dyn OverlaySurface> = Box::new(crate::surface::StubOverlay);
    assert!(!surface.is_alive());
    surface.submit_frame(OutputId(0), Arc::new(Vec::new()), 0, 0);
}
