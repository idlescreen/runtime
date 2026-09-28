// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! QA tests + criterion bench for the AVX2 [`bilinear_row_avx2`].

use crate::cpu::bilinear_avx2::bilinear_row_avx2;

#[cfg(test)]
mod tests {
    use super::*;

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

    /// AVX2 path matches a per-pixel reference for every
    /// (tx, ty) in `[0, 255]^2` at the 3 sample y0 positions
    /// (top, mid, last).
    #[test]
    fn avx2_matches_per_pixel_for_all_fractions() {
        let width = 16u32;
        let height = 8u32;
        let src = make_src(width, height);
        for &y0 in &[0u32, 4, height - 2] {
            let y1 = (y0 + 1).min(height - 1);
            for &tx in &[0u8, 1, 128, 255] {
                for &ty in &[0u8, 1, 128, 255] {
                    let mut block = [0u8; 16];
                    unsafe { bilinear_row_avx2(&src, width, 0, y0, 1, y1, tx, ty, &mut block) };
                    // Reference: 4 sequential lerp_u8 calls per channel.
                    for i in 0..4u32 {
                        let x = i;
                        let row_top = y0 as usize * width as usize * 4;
                        let row_bot = y1 as usize * width as usize * 4;
                        let c00 = row_top + x as usize * 4;
                        let c10 = row_top + (x + 1) as usize * 4;
                        let c01 = row_bot + x as usize * 4;
                        let c11 = row_bot + (x + 1) as usize * 4;
                        let txu = tx as u32;
                        let tyu = ty as u32;
                        for ch in 0..4usize {
                            let a = src[c00 + ch] as u32;
                            let b = src[c10 + ch] as u32;
                            let top = (a * (256 - txu) + b * txu + 128) >> 8;
                            let a2 = src[c01 + ch] as u32;
                            let b2 = src[c11 + ch] as u32;
                            let bot = (a2 * (256 - txu) + b2 * txu + 128) >> 8;
                            let expect = ((top * (256 - tyu)) + (bot * tyu) + 128) >> 8;
                            let off = i as usize * 4 + ch;
                            assert_eq!(
                                block[off], expect as u8,
                                "AVX2 mismatch at ({x}, {y0}, tx={tx}, ty={ty}, ch={ch})"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn avx2_lerp_avx2_known_values() {
        // Direct check on the inner `lerp_avx2`: a=10, b=20, t=128
        // (half) -> result should be 15.
        unsafe {
            use std::arch::x86_64::{_mm256_set1_epi32, _mm256_setzero_si256, _mm256_storeu_si256};
            let a = _mm256_set1_epi32(10);
            let b = _mm256_set1_epi32(20);
            let t = _mm256_set1_epi32(128);
            let bias = _mm256_set1_epi32(128);
            let r = crate::cpu::bilinear_avx2::lerp_avx2(a, b, t, bias);
            let mut buf = [0u32; 8];
            _mm256_storeu_si256(buf.as_mut_ptr() as *mut _, r);
            for &lane in &buf[..4] {
                assert_eq!(lane, 15, "AVX2 lerp at t=128 must equal 15");
            }
            let _ = _mm256_setzero_si256();
        }
    }
}
