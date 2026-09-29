// Test files legitimately panic; suppress the lint at file scope.
#![allow(clippy::panic)]
// SPDX-License-Identifier: Apache-2.0

//! Tests for the platform-agnostic idle-source contract.

use super::*;
use std::time::Duration;

#[test]
fn stub_is_unavailable() {
    assert!(!StubIdleSource::is_available());
}

#[test]
fn stub_construction_succeeds() {
    let s = StubIdleSource::new(Duration::from_secs(60)).expect("stub always constructs");
    assert!(
        !s.is_idle(),
        "stub must report not-idle so the daemon refuses to present"
    );
    assert!(
        !s.is_alive(),
        "stub must report dead so the daemon fails fast"
    );
}

#[test]
fn stub_set_timeout_is_noop() {
    let s = StubIdleSource::new(Duration::from_secs(30)).unwrap();
    s.set_timeout(Duration::from_secs(120));
    // No assertion on state — set_timeout is a no-op on the stub.
    // This test exists to anchor the public-method contract.
}

#[test]
fn platform_idle_returns_some_on_non_linux() {
    if cfg!(target_os = "linux") {
        // platform_idle returns None unconditionally on Linux by design
        // (the runner wires wayland-idle directly). Skip the assertion
        // when cross-compiling.
        return;
    }
    let handle = platform_idle(Duration::from_secs(45)).expect("non-linux must yield a stub");
    assert!(!handle.is_idle());
    assert!(!handle.is_alive());
}
