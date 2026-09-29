// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Color and scheme conversion for XDG Desktop Portal Settings.

/// Convert portal double RGB triple `(r, g, b)` in range 0.0..=1.0 to 8-bit `(u8, u8, u8)`.
pub fn portal_doubles_to_rgb(r: f64, g: f64, b: f64) -> (u8, u8, u8) {
    let clamp_u8 = |val: f64| -> u8 {
        if val.is_nan() || val <= 0.0 {
            0
        } else if val >= 1.0 {
            255
        } else {
            (val * 255.0).round() as u8
        }
    };
    (clamp_u8(r), clamp_u8(g), clamp_u8(b))
}

/// Convert portal color-scheme enum integer into an optional dark mode boolean.
///
/// XDG Desktop Portal specification for `org.freedesktop.appearance.color-scheme`:
/// - `0`: No preference (None)
/// - `1`: Prefer dark mode (Some(true))
/// - `2`: Prefer light mode (Some(false))
/// - other: Unknown (None)
pub fn portal_scheme_to_dark_mode(scheme: u32) -> Option<bool> {
    match scheme {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}
