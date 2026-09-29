// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Headless environment verification and fallback testing for wayland-present.
//!
//! Validates that when running outside an active Wayland compositor
//! (`WAYLAND_DISPLAY` is absent), connection attempts fail gracefully
//! rather than panicking or hanging.

use wayland_client::Connection;

#[test]
fn headless_env_detection_fails_gracefully() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        let res = Connection::connect_to_env();
        assert!(
            res.is_err(),
            "Connection::connect_to_env should fail when WAYLAND_DISPLAY is unset"
        );
    }
}
