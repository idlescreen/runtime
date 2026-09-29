// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Geometry helpers for layer-shell overlays (fullscreen savers vs panel inset).

use std::collections::HashMap;

use wayland_client::protocol::wl_surface;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1;

use super::types::SessionState;

/// Layer-shell margins (top, right, bottom, left). Negative expands under panel.
/// Pure — unit-tested. Half of (native − configure) on each side.
#[allow(clippy::cast_possible_wrap)]
pub fn panel_expand_margins(
    native_w: u32,
    native_h: u32,
    configured_w: u32,
    configured_h: u32,
) -> (i32, i32, i32, i32) {
    let inset_x = native_w.saturating_sub(configured_w) / 2;
    let inset_y = native_h.saturating_sub(configured_h) / 2;
    let m_y = if inset_y > 0 { -(inset_y as i32) } else { 0 };
    let m_x = if inset_x > 0 { -(inset_x as i32) } else { 0 };
    (m_y, m_x, m_y, m_x)
}

/// True when configure is strictly smaller than native mode (panel inset case).
#[allow(dead_code)] // pure helper; covered by unit tests
pub fn configure_is_panel_inset(
    native_w: u32,
    native_h: u32,
    configured_w: u32,
    configured_h: u32,
) -> bool {
    configured_w < native_w || configured_h < native_h
}

impl SessionState {
    /// Buffer / layout size for presentation.
    ///
    /// Prefer the **native output mode** when it is larger than the layer-shell
    /// configure (COSMIC often configures 1920×1040 under a panel). Savers must
    /// cover the full display including the panel bar.
    pub(crate) fn render_dimensions(
        output_id: u32,
        configured_w: u32,
        configured_h: u32,
        mode_sizes: &HashMap<u32, (u32, u32)>,
        fullscreen: bool,
    ) -> (u32, u32) {
        if !fullscreen {
            return (configured_w, configured_h);
        }
        let Some((native_w, native_h)) = mode_sizes.get(&output_id).copied() else {
            return (configured_w, configured_h);
        };
        (native_w.max(configured_w), native_h.max(configured_h))
    }

    /// Expand under the panel with **negative** margins when fullscreen and the
    /// configure is inset vs native mode.
    ///
    /// Does **not** commit — caller must `ack_configure` then commit once.
    pub(crate) fn apply_tiling_margins(
        layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        _surface: &wl_surface::WlSurface,
        output_id: u32,
        configured_w: u32,
        configured_h: u32,
        mode_sizes: &HashMap<u32, (u32, u32)>,
        fullscreen: bool,
    ) {
        if !fullscreen {
            layer_surface.set_margin(0, 0, 0, 0);
            return;
        }
        let Some((native_w, native_h)) = mode_sizes.get(&output_id).copied() else {
            layer_surface.set_margin(0, 0, 0, 0);
            return;
        };
        let (top, right, bottom, left) =
            panel_expand_margins(native_w, native_h, configured_w, configured_h);
        layer_surface.set_margin(top, right, bottom, left);
    }

    /// Exclusive zone for layer-shell: `-1` = surface wants full exclusive area
    /// (covers panel); `0` = no exclusive claim (panel may remain).
    pub(crate) fn exclusive_zone_for(fullscreen: bool) -> i32 {
        if fullscreen { -1 } else { 0 }
    }
}

#[cfg(test)]
mod geom_tests {
    use super::{SessionState, configure_is_panel_inset, panel_expand_margins};
    use std::collections::HashMap;

    #[test]
    fn fullscreen_expands_to_native_mode() {
        // Regression: preview left panel visible at 1920x1040 configure.
        let mut modes = HashMap::new();
        modes.insert(1u32, (1920u32, 1080u32));
        assert_eq!(
            SessionState::render_dimensions(1, 1920, 1040, &modes, true),
            (1920, 1080)
        );
    }

    #[test]
    fn non_fullscreen_keeps_configure() {
        let mut modes = HashMap::new();
        modes.insert(1u32, (1920u32, 1080u32));
        assert_eq!(
            SessionState::render_dimensions(1, 1920, 1040, &modes, false),
            (1920, 1040)
        );
    }

    #[test]
    fn exclusive_zone_fullscreen_is_minus_one() {
        assert_eq!(SessionState::exclusive_zone_for(true), -1);
        assert_eq!(SessionState::exclusive_zone_for(false), 0);
    }

    #[test]
    fn cosmic_panel_inset_margins_expand_under_bar() {
        // 1080 − 1040 = 40 → ±20 top/bottom (protocol negative = outward).
        let (t, r, b, l) = panel_expand_margins(1920, 1080, 1920, 1040);
        assert_eq!((t, r, b, l), (-20, 0, -20, 0));
        assert!(configure_is_panel_inset(1920, 1080, 1920, 1040));
    }

    #[test]
    fn no_margins_when_configure_matches_mode() {
        let m = panel_expand_margins(1920, 1080, 1920, 1080);
        assert_eq!(m, (0, 0, 0, 0));
        assert!(!configure_is_panel_inset(1920, 1080, 1920, 1080));
    }

    #[test]
    fn margins_zero_when_configure_larger_than_mode() {
        // Defensive: never positive insets from inverted sizes.
        let m = panel_expand_margins(1920, 1080, 1920, 1200);
        assert_eq!(m, (0, 0, 0, 0));
    }

    #[test]
    fn fullscreen_without_mode_falls_back_to_configure() {
        let modes = HashMap::new();
        assert_eq!(
            SessionState::render_dimensions(99, 1280, 720, &modes, true),
            (1280, 720)
        );
    }
}
