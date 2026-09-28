// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! QA tests + criterion bench for `stretch_byte_rows`.
//!
//! Sibling to `stretch_byte_rows.rs` so the page that defines the
//! function stays under the 256-line cap while the tests stay
//! colocated with the function they cover (RULES.md §4).

use crate::cpu::stretch_byte_rows::stretch_byte_rows;
use crate::cpu::stretch_cache::StretchCache;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upscale_2x_matches_per_pixel_for_known_grid() {
        let src_w = 4u32;
        let src_h = 2u32;
        let dst_w = 8u32;
        let dst_h = 4u32;
        let mut src = vec![0u8; (src_w as usize) * (src_h as usize) * 4];
        for y in 0..src_h {
            for x in 0..src_w {
                let i = (y as usize * src_w as usize + x as usize) * 4;
                src[i] = (x * 17 + y * 31) as u8;
                src[i + 1] = (x * 13 + y * 7) as u8;
                src[i + 2] = (x * 11 + y * 5) as u8;
                src[i + 3] = 0xFF;
            }
        }
        let mut cache = StretchCache::new();
        cache.ensure(src_w, dst_w);

        let mut dst = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
        let needed = dst.len();
        stretch_byte_rows(&mut dst, &src, src_w, src_h, dst_w, dst_h, needed, &cache);

        // Reconstruct what the naive per-pixel code would produce.
        cache.ensure(src_w, dst_w);
        let mut expected = vec![0u8; dst.len()];
        for dy in 0..dst_h as usize {
            let sy = (dy as u64 * src_h as u64 / dst_h as u64) as usize;
            let src_row = sy * src_w as usize * 4;
            let dst_row = dy * dst_w as usize * 4;
            for dx in 0..dst_w as usize {
                let sx = cache.x_map[dx] as usize;
                let so = src_row + sx * 4;
                let do_ = dst_row + dx * 4;
                expected[do_..do_ + 4].copy_from_slice(&src[so..so + 4]);
            }
        }
        assert_eq!(dst, expected, "Bresenham-span upscale must match per-pixel");
    }

    #[test]
    fn downscale_3_to_2_matches_per_pixel() {
        let src_w = 6u32;
        let src_h = 4u32;
        let dst_w = 4u32;
        let dst_h = 4u32;
        let mut src = vec![0u8; (src_w as usize) * (src_h as usize) * 4];
        for y in 0..src_h {
            for x in 0..src_w {
                let i = (y as usize * src_w as usize + x as usize) * 4;
                src[i] = (x * 17 + y * 31) as u8;
                src[i + 1] = (x * 13 + y * 7) as u8;
                src[i + 2] = (x * 11 + y * 5) as u8;
                src[i + 3] = 0xFF;
            }
        }
        let mut cache = StretchCache::new();
        cache.ensure(src_w, dst_w);

        let mut dst = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
        let needed = dst.len();
        stretch_byte_rows(&mut dst, &src, src_w, src_h, dst_w, dst_h, needed, &cache);

        cache.ensure(src_w, dst_w);
        let mut expected = vec![0u8; dst.len()];
        for dy in 0..dst_h as usize {
            let sy = (dy as u64 * src_h as u64 / dst_h as u64) as usize;
            let src_row = sy * src_w as usize * 4;
            let dst_row = dy * dst_w as usize * 4;
            for dx in 0..dst_w as usize {
                let sx = cache.x_map[dx] as usize;
                let so = src_row + sx * 4;
                let do_ = dst_row + dx * 4;
                expected[do_..do_ + 4].copy_from_slice(&src[so..so + 4]);
            }
        }
        assert_eq!(dst, expected, "Downscale must match per-pixel");
    }

    #[test]
    fn upscale_runs_lift_source_reads() {
        // Manually-paired run detection: dst_w = 8, src_w = 2.
        // 4 dst pixels per src pixel = exactly one Bresenham run of 4.
        let mut src = vec![0u8; 8];
        // pixel 0 = BGRA [10, 20, 30, 0xFF]
        src[0..4].copy_from_slice(&[10, 20, 30, 0xFF]);
        // pixel 1 = BGRA [40, 50, 60, 0xFF]
        src[4..8].copy_from_slice(&[40, 50, 60, 0xFF]);
        let mut cache = StretchCache::new();
        cache.ensure(2, 8); // dst_w = 8

        let mut dst = vec![0u8; 32];
        let needed = dst.len();
        stretch_byte_rows(&mut dst, &src, 2, 1, 8, 1, needed, &cache);

        // First 4 dst pixels (= dst_w / 2 each) should be pixel 0,
        // next 4 should be pixel 1.
        for chunk in 0..2 {
            let base = chunk * 16;
            let want = if chunk == 0 {
                [10, 20, 30, 0xFF]
            } else {
                [40, 50, 60, 0xFF]
            };
            for px in 0..4 {
                assert_eq!(&dst[base + px * 4..base + px * 4 + 4], &want);
            }
        }
    }
}

