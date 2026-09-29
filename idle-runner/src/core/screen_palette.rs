//! Backend-agnostic screen palette for r* pixel-rendered effects (GDI + console).
//!
//! **Taxonomy Classification**: System Role (Purpose - Application Software).
//!
//! `ScreenPalette` is a non-ratatui-typed bundle of RGB-tuples that describes
//! the canonical color story for a single r* app surface: background,
//! foreground, accent, dim, hot, cool, plus a few semantic channels used
//! across both console dashboards and fullscreen GDI screensavers.
//!
//! In library 4.0 the goals are:
//!
//! - A single source of truth so `helm`, `pulse`, `trance-scenes`, and
//!   future r* apps all derive their visual identity from the same place.
//! - Backend-agnostic: the struct only holds `(u8, u8, u8)` tuples. console apps
//!   can wrap the tuples in `ratatui::style::Color`; GDI apps can use them
//!   directly. No coupling between the two.
//! - Predictable: the same accent + dark-mode always produces the same palette.
//!
//! # Building a palette
//!
//! The typical flow is:
//!
//! 1. Query the system accent and dark-mode flag (via the platform
//!    helpers, e.g. `toolkit::sys_info::query_system_theme`).
//! 2. Pass both into [`ScreenPalette::from_system`] to construct the
//!    canonical 4.0 palette for the apps suite.
//! 3. Re-use the palette fields directly in your rendering code.
//!
//! # See also
//!
//! - `runner::core::hsl_to_rgb` / `rgb_to_hsl` for the math used to
//!   derive `hot` and `cool` from the accent.
//! - `runner::interface::app::effects::dimensions::Palette` for the
//!   console-typed `(u8, u8, u8)` palette used by the canonical 12 effects
//!   (FallingGlyphs, RisingFlames, etc.). A `From<&ScreenPalette>` impl
//!   bridges the two so effects can consume a `ScreenPalette` directly.

pub use idle_api::ScreenPalette;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dark_matches_cyan_ecosystem() {
        let p = ScreenPalette::default_dark();
        assert_eq!(p.accent, (0, 245, 255));
        assert_eq!(p.bg, (0, 0, 0));
        assert_eq!(p.fg, (248, 248, 242));
    }

    #[test]
    fn default_light_passes_through_accent() {
        let p = ScreenPalette::default_light();
        assert_eq!(p.accent, (0, 180, 200));
        assert_eq!(p.bg, (252, 252, 250));
    }

    #[test]
    fn from_system_preserves_accent() {
        let p = ScreenPalette::from_system((100, 200, 50), true);
        assert_eq!(p.accent, (100, 200, 50));
    }

    #[test]
    fn dim_is_scaled_accent() {
        // 0.35 factor: 100 -> 35
        let p = ScreenPalette::from_system((100, 200, 50), true);
        assert_eq!(p.dim, (35, 70, 17));
    }

    #[test]
    fn hot_and_cool_are_distinct_hues() {
        let p = ScreenPalette::from_system((255, 0, 0), true);
        // Pure red accent: hot should be near orange, cool should be far around the wheel
        assert_ne!(p.hot, p.cool);
        assert_ne!(p.hot, p.accent);
    }

    #[test]
    fn high_contrast_extremes() {
        let dark = ScreenPalette::high_contrast((0, 245, 255), true);
        assert_eq!(dark.bg, (0, 0, 0));
        assert_eq!(dark.fg, (255, 255, 255));
        let light = ScreenPalette::high_contrast((0, 245, 255), false);
        assert_eq!(light.bg, (255, 255, 255));
        assert_eq!(light.fg, (0, 0, 0));
    }

    #[test]
    fn from_system_dark_vs_light_differ() {
        let dark = ScreenPalette::from_system((100, 150, 200), true);
        let light = ScreenPalette::from_system((100, 150, 200), false);
        assert_ne!(dark.bg, light.bg);
        assert_ne!(dark.fg, light.fg);
    }
}
