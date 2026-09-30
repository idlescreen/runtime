// SPDX-License-Identifier: MIT

//! Vulkan Prime memory export to DMA-BUF file descriptors for zero-copy scanout.
//!
//! Enables direct Wayland compositor buffer imports via `linux-dmabuf` protocol,
//! avoiding intermediate CPU copy passes.

pub mod export;
pub mod types;

#[cfg(test)]
mod tests;

pub use export::{
    align_scanout_stride, export_vulkan_prime_dmabuf, is_vulkan_prime_export_available,
};
pub use types::{
    DRM_FORMAT_ARGB8888, DRM_FORMAT_MOD_LINEAR, DRM_FORMAT_XBGR2101010, DRM_FORMAT_XRGB8888,
    DmaBufPrimeExport,
};
