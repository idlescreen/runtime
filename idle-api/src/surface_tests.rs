// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: Apache-2.0

//! Tests for the platform-agnostic overlay-surface contract.

use super::*;
use std::sync::Arc;

use crate::surface::BlankAppearance;

#[test]
fn stub_is_unavailable() {
    assert!(!StubOverlay::is_available());
}

#[test]
fn stub_constructs_but_is_dead() {
    let s = StubOverlay::new().expect("stub always constructs");
    assert!(
        !s.is_alive(),
        "stub must report dead so the daemon refuses to present"
    );
    assert!(!s.is_visible());
}

#[test]
fn stub_submit_frame_is_noop() {
    let s = StubOverlay::new().unwrap();
    s.submit_frame(OutputId(0), Arc::new(Vec::new()), 0, 0);
    // No assertion on state — submit is a no-op on the stub.
}

#[test]
fn stub_show_hide_scale_layouts_are_safe_noops() {
    let s = StubOverlay::new().unwrap();
    s.show_blank(BlankAppearance::default());
    s.show_screensaver();
    s.hide();
    assert!(!s.supports_scaling());
    assert!(s.output_layouts().is_empty());
}

#[test]
fn output_id_is_hashable() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(OutputId(0));
    set.insert(OutputId(1));
    assert_eq!(set.len(), 2);
}
