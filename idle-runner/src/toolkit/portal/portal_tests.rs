// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Unit tests for portal settings parsing and conversion.

use super::accent_convert::{portal_doubles_to_rgb, portal_scheme_to_dark_mode};
use super::settings_client::query_portal_theme;

#[test]
fn test_portal_doubles_to_rgb_exact() {
    assert_eq!(portal_doubles_to_rgb(0.0, 0.0, 0.0), (0, 0, 0));
    assert_eq!(portal_doubles_to_rgb(1.0, 1.0, 1.0), (255, 255, 255));
    assert_eq!(portal_doubles_to_rgb(1.0, 0.0, 0.5), (255, 0, 128));
}

#[test]
fn test_portal_doubles_to_rgb_clamping() {
    assert_eq!(portal_doubles_to_rgb(-0.5, 1.5, 0.5), (0, 255, 128));
    assert_eq!(portal_doubles_to_rgb(f64::NAN, 0.5, 0.0), (0, 128, 0));
}

#[test]
fn test_portal_scheme_to_dark_mode() {
    assert_eq!(portal_scheme_to_dark_mode(0), None);
    assert_eq!(portal_scheme_to_dark_mode(1), Some(true));
    assert_eq!(portal_scheme_to_dark_mode(2), Some(false));
    assert_eq!(portal_scheme_to_dark_mode(99), None);
}

#[test]
fn test_query_portal_theme_graceful_fallback() {
    // Should never panic regardless of whether D-Bus is running
    let (accent, dark) = query_portal_theme();
    let _ = (accent, dark);
}
