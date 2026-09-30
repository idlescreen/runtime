// SPDX-License-Identifier: MIT

//! Wayland Color Management protocol (`wp_color_management_v1`) support.
//!
//! Handles color spaces, transfer functions, and HDR10 metadata attachment
//! for Wayland overlay presentation surfaces.

pub mod image_desc;
pub mod manager;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use image_desc::{HdrConfig, configure_hdr_overlay};
#[allow(unused_imports)]
pub use manager::{ColorManagementState, Feature, Primaries, RenderIntent, TransferFunction};
