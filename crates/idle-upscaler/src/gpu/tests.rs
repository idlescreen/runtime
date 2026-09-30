// SPDX-License-Identifier: MIT

use super::compute::{COMPUTE_STRETCH_WGSL, GpuComputeStretch};
use crate::FilterMode;

#[test]
fn compute_shader_wgsl_is_valid() {
    assert!(COMPUTE_STRETCH_WGSL.contains("@compute"));
    assert!(COMPUTE_STRETCH_WGSL.contains("cs_main"));
}

#[test]
fn compute_stretch_rejects_zero_dimensions() {
    let compute = GpuComputeStretch {
        is_available: true,
        adapter_name: Some("Test GPU".to_string()),
    };
    let mut out = vec![0u8; 16];
    let res = compute.upscale_stretch_pass(&[], 0, 0, 2, 2, FilterMode::Nearest, &mut out);
    assert!(res.is_err());
}

#[test]
fn compute_stretch_rejects_small_output_buffer() {
    let compute = GpuComputeStretch {
        is_available: true,
        adapter_name: Some("Test GPU".to_string()),
    };
    let src = vec![255u8; 16]; // 2x2
    let mut out = vec![0u8; 4]; // too small for 4x4
    let res = compute.upscale_stretch_pass(&src, 2, 2, 4, 4, FilterMode::Nearest, &mut out);
    assert!(res.is_err());
}

#[test]
fn compute_stretch_executes_valid_scaling() {
    let compute = GpuComputeStretch {
        is_available: true,
        adapter_name: Some("Test GPU".to_string()),
    };
    let mut src = vec![0u8; 16];
    // top-left red, bottom-right blue
    src[0..4].copy_from_slice(&[255, 0, 0, 255]);
    src[12..16].copy_from_slice(&[0, 0, 255, 255]);

    let mut out = vec![0u8; 64]; // 4x4
    let res = compute.upscale_stretch_pass(&src, 2, 2, 4, 4, FilterMode::Nearest, &mut out);
    assert!(res.is_ok());

    // Top-left pixel should be red
    assert_eq!(&out[0..4], &[255, 0, 0, 255]);
    // Bottom-right pixel should be blue
    assert_eq!(&out[60..64], &[0, 0, 255, 255]);
}

#[test]
fn compute_probe_does_not_panic() {
    let probe = GpuComputeStretch::probe();
    if probe.is_available {
        assert!(probe.adapter_name.is_some());
    }
}
