// SPDX-License-Identifier: MIT

//! GPU texture format selection and HDR 10-bit initialization support.

use super::scanout::types::{DRM_FORMAT_XBGR2101010, DRM_FORMAT_XRGB8888};

/// Select texture format: 10-bit HDR `Rgb10a2Unorm` or standard 8-bit `Bgra8Unorm`.
pub fn select_texture_format(enable_10bit: bool) -> wgpu::TextureFormat {
    if enable_10bit {
        wgpu::TextureFormat::Rgb10a2Unorm
    } else {
        wgpu::TextureFormat::Bgra8Unorm
    }
}

/// Detect whether the selected texture format is a 10-bit high dynamic range format.
pub fn is_10bit_format(format: wgpu::TextureFormat) -> bool {
    matches!(
        format,
        wgpu::TextureFormat::Rgb10a2Unorm | wgpu::TextureFormat::Rgb10a2Uint
    )
}

/// Map a wgpu texture format to its corresponding DRM fourcc code for scanout.
pub fn format_drm_fourcc(format: wgpu::TextureFormat) -> u32 {
    match format {
        wgpu::TextureFormat::Rgb10a2Unorm | wgpu::TextureFormat::Rgb10a2Uint => {
            DRM_FORMAT_XBGR2101010
        }
        _ => DRM_FORMAT_XRGB8888,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_format_toggles_10bit() {
        assert_eq!(
            select_texture_format(true),
            wgpu::TextureFormat::Rgb10a2Unorm
        );
        assert_eq!(
            select_texture_format(false),
            wgpu::TextureFormat::Bgra8Unorm
        );
    }

    #[test]
    fn detects_10bit_format() {
        assert!(is_10bit_format(wgpu::TextureFormat::Rgb10a2Unorm));
        assert!(!is_10bit_format(wgpu::TextureFormat::Bgra8Unorm));
    }

    #[test]
    fn drm_fourcc_mapping() {
        assert_eq!(
            format_drm_fourcc(wgpu::TextureFormat::Rgb10a2Unorm),
            DRM_FORMAT_XBGR2101010
        );
        assert_eq!(
            format_drm_fourcc(wgpu::TextureFormat::Bgra8Unorm),
            DRM_FORMAT_XRGB8888
        );
    }
}
