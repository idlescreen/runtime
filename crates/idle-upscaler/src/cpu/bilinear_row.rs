// SPDX-License-Identifier: Apache-2.0
// perf: T1 · bench: stretch · gate: perf-baseline.json
// Copyright 2026 IdleScreen

//! SIMD bilinear path for letterbox upscale.
//!
//! Single public entry point [`bilinear_row`]; per-arch
//! implementations live in sibling files:
//!
//! - [`bilinear_avx2`] — x86_64 AVX2 (`_mm256_*` intrinsics).
//! - [`bilinear_neon`] — aarch64 NEON (`vld1q_s32` / `vmulq_s32` /
//!   `vshrq_n_s32`).
//!
//! All three produce identical output to within 1 ULP per channel,
//! matching the integer lerp `lerp_u8` from [`super::sample`].

use super::sample::lerp_u8;

#[cfg(target_arch = "x86_64")]
use super::bilinear_avx2::bilinear_row_avx2;
#[cfg(target_arch = "aarch64")]
use super::bilinear_neon::bilinear_row_neon;

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

/// Reference implementation: per-pixel BGRA lerp. The Rust compiler
/// auto-vectorizes this on x86_64 with `-C target-cpu=x86-64-v3`,
/// which is the runtime default per `.cargo/config.toml`.
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

    #[test]
    fn bilinear_row_identity_at_zero_fraction() {
        // tx=0, ty=0 -> lerp collapses to the top-left source pixel.
        let src = make_src(8, 8);
        let mut block = [0u8; 16];
        bilinear_row(&src, 8, 8, 2, 3, 3, 4, 0, 0, &mut block);
        let i = 3 * 8 * 4 + 2 * 4;
        let expected = [
            src[i],
            src[i + 1],
            src[i + 2],
            src[i + 3],
            src[i + 4],
            src[i + 5],
            src[i + 6],
            src[i + 7],
            src[i + 8],
            src[i + 9],
            src[i + 10],
            src[i + 11],
            src[i + 12],
            src[i + 13],
            src[i + 14],
            src[i + 15],
        ];
        assert_eq!(block, expected);
    }
}
