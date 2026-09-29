// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Aligned u32 nearest-neighbor stretch.

use super::cache::StretchCache;

/// Nearest-neighbor stretch with both buffers viewed as `&[u32]`.
///
/// Two paths:
///
/// - **Upscale** (`dst_w >= src_w`): row-copy on duplicate source rows
///   plus Bresenham-span dst fills per row. Spans are sequential
///   `memcpy`-class throughput; the inner loop has zero divisions.
/// - **Downscale** (`dst_w < src_w`): per-pixel `x_map[dx]` lookup.
#[allow(clippy::too_many_arguments)]
pub fn stretch_u32_rows(
    src_u32: &[u32],
    dst_u32: &mut [u32],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    cache: &StretchCache,
) {
    let src_w = src_w as usize;
    let dst_w = dst_w as usize;
    if dst_w >= src_w {
        // Upscale path: sequential fills beat per-pixel gathers. Rows that
        // map to the same source row are byte-identical to the previous
        // destination row — copy_within (memcpy) instead of resampling.
        // Within a fresh row each source pixel covers a contiguous dst
        // span, so we fill spans rather than scatter per dst pixel.
        let mut prev_sy = usize::MAX;
        for dy in 0..dst_h as usize {
            let sy = dy * src_h as usize / dst_h as usize;
            let dst_start = dy * dst_w;
            let dst_end = dst_start + dst_w;
            if dst_end > dst_u32.len() {
                break;
            }
            if sy == prev_sy {
                dst_u32.copy_within(dst_start - dst_w..dst_start, dst_start);
                continue;
            }
            prev_sy = sy;
            let src_start = sy * src_w;
            if src_start + src_w > src_u32.len() {
                break;
            }
            let src_row = &src_u32[src_start..src_start + src_w];
            let dst_row = &mut dst_u32[dst_start..dst_end];
            // dst-side Bresenham: sx advances exactly when
            // (d+1)*src_w >= (sx+1)*dst_w — identical to the reference
            // gather sx = d*src_w/dst_w, but zero divisions and
            // sequential loads+stores (memcpy-class throughput).
            let mut sx = 0usize;
            let mut next_boundary = dst_w;
            let mut src_acc = 0usize;
            for d in dst_row.iter_mut() {
                *d = src_row[sx];
                src_acc += src_w;
                if src_acc >= next_boundary {
                    sx += 1;
                    next_boundary += dst_w;
                }
            }
        }
        return;
    }

    for dy in 0..dst_h as usize {
        let sy = dy * src_h as usize / dst_h as usize;
        let dst_row_start = dy * dst_w;
        let dst_row_end = dst_row_start + dst_w;
        let src_row_start = sy * src_w;

        if dst_row_end <= dst_u32.len() && src_row_start + src_w <= src_u32.len() {
            let src_row_slice = &src_u32[src_row_start..src_row_start + src_w];
            let dst_row_slice = &mut dst_u32[dst_row_start..dst_row_end];
            for (dx, val) in dst_row_slice.iter_mut().enumerate() {
                let sx = cache.x_map[dx] as usize;
                *val = src_row_slice[sx];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::cache::StretchCache;
    use super::stretch_u32_rows;

    /// u32 view of a BGRA byte buffer; requires 4-byte alignment and
    /// a multiple-of-4 length.
    // Alignment is asserted on the line below, so the u32 view is sound.
    #[allow(clippy::cast_ptr_alignment)]
    fn view_u32(bytes: &[u8]) -> &[u32] {
        let ptr = bytes.as_ptr();
        assert!(ptr.addr().is_multiple_of(4));
        assert!(bytes.len().is_multiple_of(4));
        unsafe { std::slice::from_raw_parts(ptr.cast::<u32>(), bytes.len() / 4) }
    }

    // As above: alignment and length are asserted before the cast.
    #[allow(clippy::cast_ptr_alignment)]
    fn view_u32_mut(bytes: &mut [u8]) -> &mut [u32] {
        let ptr = bytes.as_mut_ptr();
        assert!(ptr.addr().is_multiple_of(4));
        assert!(bytes.len().is_multiple_of(4));
        unsafe { std::slice::from_raw_parts_mut(ptr.cast::<u32>(), bytes.len() / 4) }
    }

    #[test]
    fn stretch_u32_rows_upscale_2x_matches_per_pixel_reference() {
        // 4×2 source, 8×4 destination. We compute the per-pixel
        // reference (gather) and check the Bresenham-span path
        // produces the same buffer.
        let src_w = 4u32;
        let src_h = 2u32;
        let dst_w = 8u32;
        let dst_h = 4u32;
        let mut src_bytes = vec![0u8; (src_w as usize) * (src_h as usize) * 4];
        for y in 0..src_h {
            for x in 0..src_w {
                let i = (y as usize * src_w as usize + x as usize) * 4;
                src_bytes[i] = (x * 17 + y * 31) as u8;
                src_bytes[i + 1] = (x * 13 + y * 7) as u8;
                src_bytes[i + 2] = (x * 11 + y * 5) as u8;
                src_bytes[i + 3] = 0xFF;
            }
        }
        let src_u32 = view_u32(&src_bytes).to_vec();

        let mut cache = StretchCache::new();
        cache.ensure(src_w, dst_w);
        let mut dst_bytes = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
        {
            let dst_u32 = view_u32_mut(&mut dst_bytes);
            stretch_u32_rows(&src_u32, dst_u32, src_w, src_h, dst_w, dst_h, &cache);
        }

        // Reference: per-pixel gather.
        let mut expected_bytes = vec![0u8; dst_bytes.len()];
        for dy in 0..dst_h as usize {
            let sy = dy * src_h as usize / dst_h as usize;
            let dst_row = dy * dst_w as usize * 4;
            let src_row = sy * src_w as usize * 4;
            for dx in 0..dst_w as usize {
                let sx = cache.x_map[dx] as usize;
                let so = src_row + sx * 4;
                let do_ = dst_row + dx * 4;
                expected_bytes[do_..do_ + 4].copy_from_slice(&src_bytes[so..so + 4]);
            }
        }
        assert_eq!(dst_bytes, expected_bytes);
    }

    #[test]
    fn stretch_u32_rows_downscale_uses_x_map() {
        // 8×2 source, 4×4 destination.
        let src_w = 8u32;
        let src_h = 2u32;
        let dst_w = 4u32;
        let dst_h = 4u32;
        let mut src_bytes = vec![0u8; (src_w as usize) * (src_h as usize) * 4];
        for y in 0..src_h {
            for x in 0..src_w {
                let i = (y as usize * src_w as usize + x as usize) * 4;
                src_bytes[i] = (x * 17 + y * 31) as u8;
                src_bytes[i + 1] = 0;
                src_bytes[i + 2] = 0;
                src_bytes[i + 3] = 0xFF;
            }
        }
        let src_u32 = view_u32(&src_bytes).to_vec();

        let mut cache = StretchCache::new();
        cache.ensure(src_w, dst_w);
        let mut dst_bytes = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
        {
            let dst_u32 = view_u32_mut(&mut dst_bytes);
            stretch_u32_rows(&src_u32, dst_u32, src_w, src_h, dst_w, dst_h, &cache);
        }

        // Sanity: each dst row uses the same source row (2 src rows / 4 dst).
        // dst_w / src_w = 0.5, so the x_map pairs dx -> sx = dx*src_w/dst_w = dx*2.
        // dx=0 -> sx=0, dx=1 -> sx=2, dx=2 -> sx=4, dx=3 -> sx=6.
        let first_dst_row_blue = dst_bytes[0];
        // src row 0, col 0 = 0x00 in our pattern (x=0,y=0).
        assert_eq!(first_dst_row_blue, 0);
        // First dst pixel src_x = 0, second dst pixel src_x = 2.
        assert_eq!(dst_bytes[4], 2 * 17);
        assert_eq!(dst_bytes[8], 4 * 17);
    }
}
