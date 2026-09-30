// SPDX-License-Identifier: MIT

use super::export::{
    align_scanout_stride, export_vulkan_prime_dmabuf, is_vulkan_prime_export_available,
};
use super::types::{
    DRM_FORMAT_ARGB8888, DRM_FORMAT_MOD_LINEAR, DRM_FORMAT_XBGR2101010, DRM_FORMAT_XRGB8888,
};

#[test]
fn stride_alignment_is_always_multiple_of_256() {
    for width in [1, 100, 640, 800, 1024, 1366, 1920, 2560, 3840, 5120] {
        let stride = align_scanout_stride(width, 4);
        assert_eq!(
            stride % 256,
            0,
            "width={width} stride={stride} not 256-byte aligned"
        );
        assert!(stride >= width * 4);
    }
}

#[test]
fn format_constants_match_drm_fourcc() {
    // DRM_FORMAT_XRGB8888 = 'X' | 'R'<<8 | '2'<<16 | '4'<<24 = 0x34325258
    let xrgb = u32::from_le_bytes(*b"XR24");
    assert_eq!(DRM_FORMAT_XRGB8888, xrgb);

    let argb = u32::from_le_bytes(*b"AR24");
    assert_eq!(DRM_FORMAT_ARGB8888, argb);

    let xbgr10 = u32::from_le_bytes(*b"XB30");
    assert_eq!(DRM_FORMAT_XBGR2101010, xbgr10);

    assert_eq!(DRM_FORMAT_MOD_LINEAR, 0);
}

#[test]
fn export_prime_dmabuf_rejects_zero_dimensions() {
    assert!(export_vulkan_prime_dmabuf(0, 1080, DRM_FORMAT_XRGB8888).is_err());
    assert!(export_vulkan_prime_dmabuf(1920, 0, DRM_FORMAT_XRGB8888).is_err());
}

#[test]
fn export_prime_dmabuf_succeeds_on_valid_input() {
    let res = export_vulkan_prime_dmabuf(1920, 1080, DRM_FORMAT_XRGB8888);
    #[cfg(target_os = "linux")]
    {
        let exp = res.expect("export dmabuf");
        assert_eq!(exp.width, 1920);
        assert_eq!(exp.height, 1080);
        assert_eq!(exp.stride % 256, 0);
        assert!(exp.size_bytes >= 1920 * 4 * 1080);
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert!(res.is_err());
    }
}

#[test]
fn probe_vulkan_prime_availability_runs() {
    let _ = is_vulkan_prime_export_available();
}
