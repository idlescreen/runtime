// SPDX-License-Identifier: MIT

//! Type definitions for Vulkan Prime DMA-BUF memory export.

use std::os::fd::OwnedFd;

/// DRM fourcc format codes for scanout buffers.
pub const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;
pub const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;
pub const DRM_FORMAT_ABGR2101010: u32 = 0x3033_4241;
pub const DRM_FORMAT_XBGR2101010: u32 = 0x3033_4258;

/// Linear modifier for hardware scanout compatibility.
pub const DRM_FORMAT_MOD_LINEAR: u64 = 0;

/// Exported Vulkan Prime DMA-BUF memory descriptor.
#[derive(Debug)]
pub struct DmaBufPrimeExport {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub drm_format: u32,
    pub modifier: u64,
    pub size_bytes: usize,
}

impl DmaBufPrimeExport {
    pub fn new(
        fd: OwnedFd,
        width: u32,
        height: u32,
        stride: u32,
        drm_format: u32,
        modifier: u64,
        size_bytes: usize,
    ) -> Self {
        Self {
            fd,
            width,
            height,
            stride,
            drm_format,
            modifier,
            size_bytes,
        }
    }
}
