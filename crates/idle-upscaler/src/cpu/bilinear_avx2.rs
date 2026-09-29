// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! AVX2 (x86_64) bilinear row implementation.
//!
//! Processes 4 horizontally-adjacent BGRA pixels per call via 8-lane
//! `__m256i` vectors. Native i32 multiply (`_mm256_mullo_epi32`) +
//! per-lane arithmetic shift (`_mm256_srav_epi32`) avoids both the
//! SSE2 sign-extension trap AND the manual sign-mask workaround
//! that produced off-by-1 errors for negative intermediates.

#![cfg(target_arch = "x86_64")]
// Every function below is already an `unsafe fn` documenting its own
// `# Safety` precondition, and each intrinsic is called exactly where
// that precondition is discharged. Re-stating it as an `unsafe {}`
// block per call would add ~20 lines of ceremony to a 176-line page
// without changing what is checked.
#![allow(unsafe_op_in_unsafe_fn)]

use std::arch::x86_64::{
    __m256i, _mm256_add_epi32, _mm256_mullo_epi32, _mm256_set1_epi32, _mm256_setr_epi32,
    _mm256_srav_epi32, _mm256_storeu_si256, _mm256_sub_epi32,
};

/// Read the channel-c byte of each of 4 horizontally-adjacent
/// pixels in BGRA order, where `offset = base_byte + c` is the
/// byte offset of channel c for pixel 0. Returns `[ch0, ch1,
/// ch2, ch3]` indexed by pixel.
///
/// Bytes are stride-4 in `src` (one pixel per 4 bytes), so we
/// issue four separate byte reads and combine.
///
/// # Safety
/// `offset..offset+16` is the 16-byte window covering 4 BGRA
/// pixels in stride-4 order; the caller must ensure it's in-bounds.
#[inline]
unsafe fn load_channel_u8s(src: &[u8], offset: usize) -> [u8; 4] {
    [
        *src.as_ptr().add(offset),
        *src.as_ptr().add(offset + 4),
        *src.as_ptr().add(offset + 8),
        *src.as_ptr().add(offset + 12),
    ]
}

/// `result = a + ((b - a) * t + 128) >>_arith 8` per i32 lane.
///
/// Uses `_mm256_srav_epi32` (AVX2-native per-lane arithmetic
/// right shift) which matches Rust's native `i32 >> i32` (also
/// arithmetic shift). Don't try to emulate this with
/// `_mm256_srli_epi32` + sign-mask OR — that produces an
/// `OR` of all-ones into every negative lane, off by ~256 ULP.
#[inline]
unsafe fn lerp_avx2(a: __m256i, b: __m256i, t: __m256i, bias: __m256i) -> __m256i {
    let diff = _mm256_sub_epi32(b, a);
    let scaled = _mm256_mullo_epi32(diff, t);
    let biased = _mm256_add_epi32(scaled, bias);
    // Per-lane arithmetic shift right by 8 — sign bit
    // replicated, matching Rust `biased >> 8`.
    let shifted = _mm256_srav_epi32(biased, _mm256_set1_epi32(8));
    _mm256_add_epi32(a, shifted)
}

#[inline]
#[allow(clippy::cast_ptr_alignment)]
unsafe fn extract_lane(v: __m256i, lane: usize) -> u32 {
    // SAFETY: `lane < 4` is asserted at every call site; lanes
    // 0..3 are valid u32 reads from `buf` (lanes 4..7 are also
    // valid but hold 0). `_mm256_storeu_si256` accepts an
    // unaligned pointer — `[u32; 8]` is 4-byte aligned, which
    // is sufficient for storeu (but not for store, hence the
    // alignment-cast warning we're suppressing here).
    debug_assert!(lane < 4);
    let mut buf = [0u32; 8];
    _mm256_storeu_si256(buf.as_mut_ptr() as *mut __m256i, v);
    buf[lane]
}

/// AVX2 bilinear row. See module docs for the math; called via
/// [`bilinear_row`](super::bilinear_row) when the runtime
/// `avx2` feature probe succeeds.
///
/// # Safety
/// Caller bounds-checks `src` for the 16-byte windows loaded for
/// each channel of 4 horizontally-adjacent pixels.
#[target_feature(enable = "avx2")]
pub unsafe fn bilinear_row_avx2(
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

    // `pixel_results[channel][pixel]`: 4 channels × 4 pixels.
    let mut pixel_results: [[u32; 4]; 4] = [[0; 4]; 4];

    for (c, channel) in pixel_results.iter_mut().enumerate() {
        let c00 = y0 as usize * row_stride + x0 as usize * 4;
        let c10 = y0 as usize * row_stride + x1 as usize * 4;
        let c01 = y1 as usize * row_stride + x0 as usize * 4;
        let c11 = y1 as usize * row_stride + x1 as usize * 4;

        let c00_bytes = load_channel_u8s(src, c00 + c);
        let c10_bytes = load_channel_u8s(src, c10 + c);
        let c01_bytes = load_channel_u8s(src, c01 + c);
        let c11_bytes = load_channel_u8s(src, c11 + c);

        // Build i32 vectors. Lanes 0..3 carry the 4 pixels'
        // channel-c values; lanes 4..7 stay at zero (the
        // lerp propagates 128+0 -> 0 through them).
        let a = _mm256_setr_epi32(
            c00_bytes[0] as i32,
            c00_bytes[1] as i32,
            c00_bytes[2] as i32,
            c00_bytes[3] as i32,
            0,
            0,
            0,
            0,
        );
        let b = _mm256_setr_epi32(
            c10_bytes[0] as i32,
            c10_bytes[1] as i32,
            c10_bytes[2] as i32,
            c10_bytes[3] as i32,
            0,
            0,
            0,
            0,
        );
        let a2 = _mm256_setr_epi32(
            c01_bytes[0] as i32,
            c01_bytes[1] as i32,
            c01_bytes[2] as i32,
            c01_bytes[3] as i32,
            0,
            0,
            0,
            0,
        );
        let b2 = _mm256_setr_epi32(
            c11_bytes[0] as i32,
            c11_bytes[1] as i32,
            c11_bytes[2] as i32,
            c11_bytes[3] as i32,
            0,
            0,
            0,
            0,
        );

        let bias = _mm256_set1_epi32(128);
        let top = lerp_avx2(a, b, _mm256_set1_epi32(tx as i32), bias);
        let bot = lerp_avx2(a2, b2, _mm256_set1_epi32(tx as i32), bias);
        let result = lerp_avx2(top, bot, _mm256_set1_epi32(ty as i32), bias);

        for (i, slot) in channel.iter_mut().enumerate() {
            *slot = extract_lane(result, i);
        }
    }

    // Interleave per-channel results into BGRA byte layout:
    // `out[i*4 + c] = pixel_results[c][i]` as u8.
    for (c, channel) in pixel_results.iter().enumerate() {
        for (i, &val) in channel.iter().enumerate() {
            out[i * 4 + c] = val as u8;
        }
    }
}

// Tests + bench live in `bilinear_avx2_tests.rs` so the page that
// defines the SIMD intrinsics stays under the 256-line cap.
#[cfg(test)]
#[path = "bilinear_avx2_tests.rs"]
mod tests;
