// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

//! Lightweight shared API definitions, traits, and math utilities for terminal
//! screensaver plugins. Host applications register callbacks for live system
//! queries; plugins depend only on this crate for portable drawing primitives.
//!
//! ## Example
//!
//! ```rust
//! use idle_api::{LcgRng, Screensaver, TerminalCell};
//!
//! struct SolidRed;
//!
//! impl Screensaver for SolidRed {
//!     fn update(&mut self, _: std::time::Duration, _: usize, _: usize) {}
//!     fn draw(&self, grid: &mut [TerminalCell], _cols: usize, _rows: usize) {
//!         for cell in grid.iter_mut() {
//!             cell.bg = (255, 0, 0); // pure red
//!             cell.fg = (255, 0, 0);
//!         }
//!     }
//! }
//!
//! let mut rng = LcgRng::new(0xDEAD_BEEF);
//! let _ = rng.next_u64();
//! ```

/// Host/plugin API major version. Plugins may export `idle_api_version() -> u32`
/// (legacy `trance_api_version` still accepted by the loader when present).
pub const API_VERSION: u32 = 1;

pub mod asset;
pub mod audio;
pub mod c_abi;
mod callbacks;
pub mod caption;
pub mod color;
mod env_dual;
pub mod idle_source;
mod logo_block;
pub mod monitor;
pub mod rng;
mod screensaver;
pub mod stress;
pub mod surface;
pub mod toml;

pub use asset::{ASSET_LOGO, asset, env_key as asset_env_key, logo};
pub use env_dual::{
    SAVER_PARAM_ENV_PREFIX, env_is_set, env_truthy, env_var_first, param, param_f32,
    saver_param_env_key, set_env,
};
mod system_info;

pub use audio::{AudioBands, query_audio_bands};
pub use c_abi::{CAbiSaver, IdleCell, IdleSaverOps, OPS_SYMBOL};
pub use callbacks::{
    PALETTE_CALLBACK, SYSTEM_INFO_CALLBACK, get_system_info, query_current_palette,
};
pub use caption::{caption_text, clear_caption, publish_caption, with_caption};
pub use color::{ScreenPalette, Theme, hsl_to_rgb, lerp, percentage, rgb_to_hsl};
pub use idle_source::{IdleSource, StubIdleSource, platform_idle};
pub use logo_block::render_logo_block;
pub use monitor::{
    IS_SECONDARY_MONITOR_CALLBACK, MONITOR_BOUNDS_CALLBACK, MonitorCellBounds,
    clear_primary_bounds, get_primary_monitor_bounds, is_secondary_monitor, publish_primary_bounds,
};
pub use plugin_manifest::signature::{signature_path, signature_required, verify_signature};
pub use rng::{LcgRng, SEED_ENV_KEYS, seed_from_env};
pub use screensaver::{GpuSpotlight, Screensaver, ScreensaverInstance, ScreensaverState};
#[cfg(target_os = "linux")]
pub use surface::WaylandOverlay;
pub use surface::{
    BlankAppearance, CenteredLogo, OutputId, OutputLayout, OverlaySurface, StubOverlay,
    TerminalCell, is_span_layout, place_centered_logo, span_reach_scale,
};
pub use system_info::SystemInfo;

/// `.idleplugin.toml` capability manifest (schema v1).
pub mod plugin_manifest;
