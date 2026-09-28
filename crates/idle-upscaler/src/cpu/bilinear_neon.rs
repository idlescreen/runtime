// SPDX-License-Identifier: Apache-2.0
// perf: T2 · bench: none · ubuntu-latest is x86_64; promotion to T1 needs an aarch64 runner
// Copyright 2026 IdleScreen

//! NEON (aarch64) bilinear row implementation.
//!
//! Processes 4 horizontally-adjacent BGRA pixels per call via
//! 4-lane `int32x4_t` vectors. `vmulq_s32` for the i32 multiply,
//! `vshrq_n_s32::<8>` for the signed shift.

#![cfg(target_arch = "aarch64")]

use std::arch::aarch64::{
    int32x4_t, vaddq_s32, vdupq_n_s32, vld1q_s32, vmulq_s32, vshrq_n_s32, vst1q_s32, vsubq_s32,
};

/// Read the channel-c byte of each of 4 horizontally-adjacent
/// BGRA pixels and promote to a `int32x4_t` with one u8 per lane.
///
/// `offset = base_byte + c` is the byte offset of channel c for
/// pixel 0; pixels are stride-4.
///
/// # Safety
/// Caller bounds-checks `src` for the 16-byte window.
#[inline]
unsafe fn load_channel_i32x4(src: &[u8], offset: usize) -> int32x4_t {
    let bytes = [
        *src.as_ptr().add(offset) as i32,
        *src.as_ptr().add(offset + 4) as i32,
        *src.as_ptr().add(offset + 8) as i32,
        *src.as_ptr().add(offset + 12) as i32,
    ];
    vld1q_s32(bytes.as_ptr() as *const i32)
}

/// `result = a + ((b - a) * t + 128) >>_arith 8` per i32 lane.
///
/// `vshrq_n_s32::<8>` is the signed (arithmetic) right shift by
/// immediate 8 — sign bit replicated, matching Rust's `i32 >> 8`.
#[inline]
unsafe fn lerp_neon(a: int32x4_t, b: int32x4_t, t: int32x4_t, bias: int32x4_t) -> int32x4_t {
    let diff = vsubq_s32(b, a);
    let scaled = vmulq_s32(diff, t);
    let biased = vaddq_s32(scaled, bias);
    let shifted = vshrq_n_s32::<8>(biased);
    vaddq_s32(a, shifted)
}

/// NEON bilinear row. See module docs for the math; called via
/// [`bilinear_row`](super::bilinear_row) when the runtime
/// `neon` feature probe succeeds.
///
/// # Safety
/// Caller bounds-checks `src` for the 16-byte windows loaded for
/// each channel of 4 horizontally-adjacent pixels.
#[target_feature(enable = "neon")]
pub unsafe fn bilinear_row_neon(
    src: &[u8],
    width: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    tx: u8,
    ty: u8,
    out: &mut [u8; 16],
) {
    let row_stride = width as usize * 4;

    let tx_v = vdupq_n_s32(tx as i32);
    let ty_v = vdupq_n_s32(ty as i32);
    let bias = vdupq_n_s32(128);

    let mut pixel_results: [[i32; 4]; 4] = [[0; 4]; 4];

    for c in 0..4usize {
        let c00 = y0 as usize * row_stride + x0 as usize * 4;
        let c10 = y0 as usize * row_stride + x1 as usize * 4;
        let c01 = y1 as usize * row_stride + x0 as usize * 4;
        let c11 = y1 as usize * row_stride + x1 as usize * 4;

        let a = load_channel_i32x4(src, c00 + c);
        let b = load_channel_i32x4(src, c10 + c);
        let a2 = load_channel_i32x4(src, c01 + c);
        let b2 = load_channel_i32x4(src, c11 + c);

        let top = lerp_neon(a, b, tx_v, bias);
        let bot = lerp_neon(a2, b2, tx_v, bias);
        let result = lerp_neon(top, bot, ty_v, bias);

        let mut lanes = [0i32; 4];
        vst1q_s32(lanes.as_mut_ptr(), result);
        pixel_results[c] = lanes;
    }

    for (c, channel) in pixel_results.iter().enumerate() {
        for (i, &val) in channel.iter().enumerate() {
            out[i * 4 + c] = val as u8;
        }
    }
}

// Tests + bench live in `bilinear_neon_tests.rs` so the page that
// defines the SIMD intrinsics stays under the 256-line cap.
#[cfg(test)]
#[path = "bilinear_neon_tests.rs"]
mod tests;
