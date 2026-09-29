//! Core shared types and primitives. Vendored from `runner::core`.
//! Source: /home/jeryd/library/src/core/mod.rs (and included submodules).
//!
//! This module stays free of host-specific dependencies so dynamically loaded
//! screensaver plugins can import palette and logo helpers without pulling in
//! the full runner stack.
//!
//! ## Submodules
//!
//! - [`logo_block`] — 5×5 block-letter OS logo renderer with cache
//! - [`screen_palette`] — accent-driven terminal color palette
//!
//! Screensaver traits are re-exported from [`idle_api`] for plugin compatibility.
//! The nested [`screensaver`] module preserves historical import paths.

pub mod logo_block;
pub mod screen_palette;

pub use idle_api::{
    LcgRng, Screensaver, ScreensaverState, TerminalCell, hsl_to_rgb, lerp, percentage, rgb_to_hsl,
};

/// Compatibility namespace matching historical `core::screensaver` imports.
pub mod screensaver {
    pub use idle_api::{Screensaver, ScreensaverState};
}
