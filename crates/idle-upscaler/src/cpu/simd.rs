// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! SIMD bilinear path for letterbox upscale.
//!
//! Three implementations behind a single public entry point
//! [`bilinear_row`]:
//!
//! - **AVX2 (x86_64)** — 8 i32 lanes per `__m256i`, processes 4 BGRA
//!   pixels per call. Native i32 multiply (`_mm256_mullo_epi32`) +
//!   per-lane arithmetic shift (`_mm256_srav_epi32`) avoids both the
//!   SSE2 sign-extension trap AND the manual sign-mask workaround
//!   that produced off-by-1 errors for negative intermediates.
//! - **NEON (aarch64)** — 4 i32 lanes per `int32x4_t`, processes
//!   4 BGRA pixels per call. `vmulq_s32` for the i32 multiply,
//!   `vshrq_n_s32::<8>` for the signed shift.
//! - **Scalar** — used on every other target. The Rust compiler
//!   auto-vectorizes on x86_64 with `-C target-cpu=x86-64-v3`.
//!
//! All three produce identical output to within 1 ULP per channel,
//! matching the integer lerp `lerp_u8` from [`super::sample`].

use super::sample::lerp_u8;

/// Bilinear sample of 4 horizontally-adjacent BGRA pixels at the
/// same fractional y. Writes 16 bytes (4 × BGRA) to `out`.
///
/// The caller is responsible for bounds-checks on `src`. `tx` and
/// `ty` are 8-bit fractional offsets in `[0, 255]`.
pub fn bilinear_row(
    src: &[u8],
    width: u32,
    height: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    tx: u8,
    ty: u8,
    out: &mut [u8; 16],
) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe { bilinear_row_avx2(src, width, x0, y0, x1, y1, tx, ty, out) };
            return;
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            unsafe { bilinear_row_neon(src, width, x0, y0, x1, y1, tx, ty, out) };
            return;
        }
    }

    bilinear_row_scalar(src, width, height, x0, y0, x1, y1, tx, ty, out);
}

fn bilinear_row_scalar(
    src: &[u8],
    width: u32,
    _height: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    tx: u8,
    ty: u8,
    out: &mut [u8; 16],
) {
    let row_stride = width as usize * 4;

    for i in 0..4usize {
        let src_x = x0 as usize + i;
        let src_x1 = x1 as usize + i;
        let c00 = y0 as usize * row_stride + src_x * 4;
        let c10 = y0 as usize * row_stride + src_x1 * 4;
        let c01 = y1 as usize * row_stride + src_x * 4;
        let c11 = y1 as usize * row_stride + src_x1 * 4;

        for c in 0..4usize {
            let a = src[c00 + c];
            let b = src[c10 + c];
            let top = lerp_u8(a, b, tx);
            let a2 = src[c01 + c];
            let b2 = src[c11 + c];
            let bottom = lerp_u8(a2, b2, tx);
            out[i * 4 + c] = lerp_u8(top, bottom, ty);
        }
    }
}

// =================================================================
// AVX2 (x86_64)
// =================================================================

#[cfg(target_arch = "x86_64")]
mod avx2 {
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
    #[inline]
    fn load_channel_u8s(src: &[u8], offset: usize) -> [u8; 4] {
        // SAFETY: `offset..offset+16` is the 16-byte window covering
        // 4 BGRA pixels in stride-4 order, which the caller must
        // ensure is in-bounds.
        unsafe {
            [
                *src.as_ptr().add(offset),
                *src.as_ptr().add(offset + 4),
                *src.as_ptr().add(offset + 8),
                *src.as_ptr().add(offset + 12),
            ]
        }
    }

    /// `result = a + ((b - a) * t + 128) >>_arith 8` per i32 lane.
    ///
    /// Uses `_mm256_srav_epi32` (AVX2-native per-lane arithmetic
    /// right shift) which matches Rust's native `i32 >> i32` (also
    /// arithmetic shift). Don't try to emulate this with
    /// `_mm256_srli_epi32` + sign-mask OR — that produces an
    /// `OR` of all-ones into every negative lane, off by ~256 ULP.
    #[inline]
    fn lerp_avx2(a: __m256i, b: __m256i, t: __m256i, bias: __m256i) -> __m256i {
        unsafe {
            let diff = _mm256_sub_epi32(b, a);
            let scaled = _mm256_mullo_epi32(diff, t);
            let biased = _mm256_add_epi32(scaled, bias);
            // Per-lane arithmetic shift right by 8 — sign bit
            // replicated, matching Rust `biased >> 8`.
            let shifted = _mm256_srav_epi32(biased, _mm256_set1_epi32(8));
            _mm256_add_epi32(a, shifted)
        }
    }

    #[inline]
    #[allow(clippy::cast_ptr_alignment)]
    fn extract_lane(v: __m256i, lane: usize) -> u32 {
        // SAFETY: `lane < 4` is asserted at every call site; lanes
        // 0..3 are valid u32 reads from `buf` (lanes 4..7 are also
        // valid but hold 0). `_mm256_storeu_si256` accepts an
        // unaligned pointer — `[u32; 8]` is 4-byte aligned, which
        // is sufficient for storeu (but not for store, hence the
        // alignment-cast warning we're suppressing here).
        debug_assert!(lane < 4);
        let mut buf = [0u32; 8];
        unsafe {
            _mm256_storeu_si256(buf.as_mut_ptr() as *mut __m256i, v);
        }
        buf[lane]
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn bilinear_row(
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
}

// =================================================================
// NEON (aarch64)
// =================================================================

#[cfg(target_arch = "aarch64")]
mod neon {
    use std::arch::aarch64::{
        int32x4_t, vaddq_s32, vdupq_n_s32, vld1q_s32, vmulq_s32, vshrq_n_s32, vst1q_s32, vsubq_s32,
    };

    /// Read the channel-c byte of each of 4 horizontally-adjacent
    /// BGRA pixels and promote to a `int32x4_t` with one u8 per lane.
    ///
    /// `offset = base_byte + c` is the byte offset of channel c for
    /// pixel 0; pixels are stride-4.
    #[inline]
    fn load_channel_i32x4(src: &[u8], offset: usize) -> int32x4_t {
        // SAFETY: caller bounds-checks `src` for the 16-byte window.
        let bytes = unsafe {
            [
                *src.as_ptr().add(offset) as i32,
                *src.as_ptr().add(offset + 4) as i32,
                *src.as_ptr().add(offset + 8) as i32,
                *src.as_ptr().add(offset + 12) as i32,
            ]
        };
        unsafe { vld1q_s32(bytes.as_ptr() as *const i32) }
    }

    /// `result = a + ((b - a) * t + 128) >>_arith 8` per i32 lane.
    ///
    /// `vshrq_n_s32::<8>` is the signed (arithmetic) right shift by
    /// immediate 8 — sign bit replicated, matching Rust's `i32 >> 8`.
    #[inline]
    fn lerp_neon(a: int32x4_t, b: int32x4_t, t: int32x4_t, bias: int32x4_t) -> int32x4_t {
        unsafe {
            let diff = vsubq_s32(b, a);
            let scaled = vmulq_s32(diff, t);
            let biased = vaddq_s32(scaled, bias);
            let shifted = vshrq_n_s32::<8>(biased);
            vaddq_s32(a, shifted)
        }
    }

    #[target_feature(enable = "neon")]
    pub unsafe fn bilinear_row(
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

        unsafe {
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
    }
}

// =================================================================
// Public-entry wrappers (one per arch)
// =================================================================

#[cfg(target_arch = "x86_64")]
unsafe fn bilinear_row_avx2(
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
    unsafe { avx2::bilinear_row(src, width, x0, y0, x1, y1, tx, ty, out) }
}

#[cfg(target_arch = "aarch64")]
unsafe fn bilinear_row_neon(
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
    unsafe { neon::bilinear_row(src, width, x0, y0, x1, y1, tx, ty, out) }
}

#[cfg(test)]
mod tests {
    use super::bilinear_row;
    use crate::cpu::sample::sample_bilinear;

    fn make_src(width: u32, height: u32) -> Vec<u8> {
        let mut buf = vec![0u8; (width as usize) * (height as usize) * 4];
        for y in 0..height {
            for x in 0..width {
                let v = ((x.wrapping_mul(17) ^ y.wrapping_mul(31)) & 0xFF) as u8;
                let i = (y as usize * width as usize + x as usize) * 4;
                buf[i] = v;
                buf[i + 1] = v.wrapping_add(7);
                buf[i + 2] = v.wrapping_add(13);
                buf[i + 3] = 0xFF;
            }
        }
        buf
    }

    #[test]
    fn bilinear_row_matches_scalar_for_random_fractions() {
        let width = 64u32;
        let height = 32u32;
        let src = make_src(width, height);

        for &y0 in &[0u32, 5, height - 2] {
            let y1 = (y0 + 1).min(height - 1);
            for ty in [0u8, 17, 64, 128, 200, 255] {
                for x0 in 0..(width - 4) {
                    let x1 = (x0 + 1).min(width - 1);
                    for tx in [0u8, 1, 17, 64, 128, 200, 254, 255] {
                        let mut block = [0u8; 16];
                        bilinear_row(&src, width, height, x0, y0, x1, y1, tx, ty, &mut block);

                        for i in 0..4u32 {
                            let x = x0 + i;
                            let scalar = sample_bilinear(
                                &src,
                                width,
                                height,
                                x as f32 + tx as f32 / 256.0,
                                y0 as f32 + ty as f32 / 256.0,
                            );
                            for (c, &sv) in scalar.iter().enumerate() {
                                let off = (i as usize) * 4 + c;
                                assert_eq!(
                                    block[off], sv,
                                    "SIMD mismatch at ({x}, {y0}, tx={tx}, ty={ty}, channel {c}): simd={}, scalar={sv}",
                                    block[off]
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
