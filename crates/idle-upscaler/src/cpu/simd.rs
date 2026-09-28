// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! SIMD bilinear path for letterbox upscale.
//!
//! Status: **experimental**. The original goal was an SSE2 path that
//! processed 4 BGRA pixels per `__m128i` iteration. The challenge is
//! the 32-bit intermediate `(b - a) * t + 128` — for u8 channels, this
//! lives in `[-65025, 65025]`, which overflows the i16 lanes that
//! SSE2's PMULLW/PMULHW operate on.
//!
//! A correct SSE2 implementation needs the i32 product — combine
//! `_mm_mullo_epi16` (low bits) + `_mm_mulhi_epi16` (high bits) and
//! reconstruct `(hi as i32) << 16 | (lo as u16)` as a full i32.
//! The SSE2-only path then needs to manage the sign extension of
//! `hi`: for products that fit in i16, `hi` is the sign-extension
//! `-1` (not `0`), so a naive `hi * 65536 + lo` reconstruction
//! overshoots by ~256. Getting this right requires either:
//!
//! - A pre-check that the product fits in i16 (branch in SIMD =
//!   awkward), or
//! - Promoting to `__m256i` (AVX2) for native i32 arithmetic — not
//!   universally available.
//!
//! Until that the is solved, [`bilinear_row`] falls back to the
//! scalar integer lerp from [`super::sample::lerp_u8`], which gives
//! correct output for all input combinations. The compiler
//! auto-vectorizes the 4-pixel loop nicely on x86_64 with
//! `-C target-cpu=x86-64-v3` (or `-C target-feature=+sse2` etc.) —
//! the per-pixel work becomes straight-line u8 arithmetic that
//! LLVM lowers to packed ops.
//!
//! The entry point [`bilinear_row`] is wired into `letterbox.rs` and
//! produces identical output to the previous scalar `sample_src`
//! path (verified by `cpu::simd::tests::bilinear_row_matches_scalar`).

use super::sample::lerp_u8;

/// Bilinear sample of 4 horizontally-adjacent BGRA pixels at the
/// same fractional y. Writes 16 bytes (4 × BGRA) to `out`.
///
/// On x86_64 (and aarch64, musl, etc.) without a working SSE2 path,
/// this currently calls [`lerp_u8`] 16 times — the compiler
/// auto-vectorizes the inner loop. A real SSE2 path would process
/// 4 pixels per `__m128i` iteration; see the module docs for why
/// the SSE2 path is held.
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
    let row_stride = width as usize * 4;
    let _ = height;

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
}
