// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Nearest-neighbor stretch upscale (fills destination, may distort aspect).

/// Checked `&[u8]` → `&[u32]` view (bytemuck `try_cast_slice` replacement):
/// requires 4-byte alignment and a multiple-of-4 length.
#[allow(clippy::cast_ptr_alignment)]
fn try_cast_u8_to_u32(src: &[u8]) -> Result<&[u32], ()> {
    if !src.len().is_multiple_of(4) || !src.as_ptr().addr().is_multiple_of(4) {
        return Err(());
    }
    Ok(unsafe { std::slice::from_raw_parts(src.as_ptr().cast::<u32>(), src.len() / 4) })
}

/// Mutable counterpart of [`try_cast_u8_to_u32`].
#[allow(clippy::cast_ptr_alignment)]
fn try_cast_u8_to_u32_mut(dst: &mut [u8]) -> Result<&mut [u32], ()> {
    if !dst.len().is_multiple_of(4) || !dst.as_ptr().addr().is_multiple_of(4) {
        return Err(());
    }
    Ok(unsafe { std::slice::from_raw_parts_mut(dst.as_mut_ptr().cast::<u32>(), dst.len() / 4) })
}

/// Cached nearest-neighbor column map for stretch upscale.
pub struct StretchCache {
    /// Source width last used to build `x_map` (test/cache introspection).
    pub src_w: u32,
    /// Destination width last used to build `x_map`.
    pub dst_w: u32,
    /// Nearest-neighbor source-x for each destination column.
    pub x_map: Vec<u32>,
}

impl StretchCache {
    pub fn new() -> Self {
        Self {
            src_w: 0,
            dst_w: 0,
            x_map: Vec::new(),
        }
    }

    pub fn ensure(&mut self, src_w: u32, dst_w: u32) {
        if self.src_w == src_w && self.dst_w == dst_w && self.x_map.len() == dst_w as usize {
            return;
        }
        self.src_w = src_w;
        self.dst_w = dst_w;
        self.x_map = (0..dst_w)
            .map(|dx| (dx as u64 * src_w as u64 / dst_w as u64) as u32)
            .collect();
    }
}

/// Fast integer nearest-neighbor stretch into `dst` (reuses `cache` x-map).
#[allow(clippy::too_many_arguments)]
pub fn upscale_stretch_into(
    dst: &mut [u8],
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    cache: &mut StretchCache,
) {
    let needed = (dst_w as usize)
        .checked_mul(dst_h as usize)
        .and_then(|p| p.checked_mul(4))
        .unwrap_or(0);
    if dst.len() < needed {
        return;
    }
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        dst[..needed].fill(0);
        return;
    }

    if src_w == dst_w && src_h == dst_h {
        let copy_len = needed.min(src.len());
        dst[..copy_len].copy_from_slice(&src[..copy_len]);
        if needed > copy_len {
            dst[copy_len..needed].fill(0);
        }
        return;
    }

    match (
        try_cast_u8_to_u32(src),
        try_cast_u8_to_u32_mut(&mut dst[..needed]),
    ) {
        (Ok(src_u32), Ok(dst_u32)) => {
            if dst_w < src_w {
                cache.ensure(src_w, dst_w);
            }
            stretch_u32_rows(src_u32, dst_u32, src_w, src_h, dst_w, dst_h, cache)
        }
        _ => {
            cache.ensure(src_w, dst_w);
            stretch_byte_rows(dst, src, src_w, src_h, dst_w, dst_h, needed, cache)
        }
    }
}

fn stretch_u32_rows(
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

#[allow(clippy::too_many_arguments)]
fn stretch_byte_rows(
    dst: &mut [u8],
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    // Retained in the signature so the call site stays uniform across the
    // aligned (`stretch_u32_rows`) and unaligned byte paths; was previously
    // used by `dst[..needed].fill(0)` before that memset was dropped.
    _needed: usize,
    cache: &StretchCache,
) {
    // Fallback unaligned byte-copy path. The destination pixels are
    // 4 bytes apart and the source pixels are 4 bytes apart, so a true
    // SIMD-batchable memcpy isn't possible (we can't grow the slice
    // into a single contiguous span since the rows are independent).
    // Each dst pixel still costs one 4-byte `copy_from_slice`, but we
    // hoist the row-end bounds checks + row-base offsets to once per
    // row, AND run-length-compress consecutive dst pixels that share
    // the same source pixel (Bresenham span). The latter is a real
    // win for upscale (dst_w >> src_w): one source 4-byte load
    // services many dst pixels. Downscale (dst_w < src_w) sees no
    // benefit — every dst pixel gets a distinct source pixel — and
    // falls back to the per-pixel copy.
    let dst_w_us = dst_w as usize;
    let src_w_us = src_w as usize;
    let src_row_bytes = src_w_us * 4;
    let dst_row_bytes = dst_w_us * 4;
    let upscale = dst_w >= src_w;

    for dy in 0..dst_h as usize {
        let sy = (dy as u64 * src_h as u64 / dst_h as u64) as usize;
        let src_row_base = sy * src_row_bytes;
        let dst_row_base = dy * dst_row_bytes;
        if src_row_base + src_row_bytes > src.len() {
            break;
        }
        if dst_row_base + dst_row_bytes > dst.len() {
            break;
        }
        if upscale {
            emit_row_upscale(dst, src, dst_row_base, src_row_base, cache, dst_w_us);
        } else {
            emit_row_downscale(dst, src, dst_row_base, src_row_base, cache, dst_w_us);
        }
    }
}

/// Upscale Bresenham-span emit: scan the cached x_map once per row,
/// grouping consecutive dst pixels that share the same source pixel.
/// For runs of length ≥ 2 we read the source once and broadcast-copy
/// it to every dst pixel in the run. For runs of length 1 (1:1 or
/// near-1:1 scale), this matches the per-pixel code exactly.
fn emit_row_upscale(
    dst: &mut [u8],
    src: &[u8],
    dst_row_base: usize,
    src_row_base: usize,
    cache: &StretchCache,
    dst_w_us: usize,
) {
    let mut run_sx: usize = cache.x_map[0] as usize;
    let mut run_start: usize = 0;
    for dx in 1..dst_w_us {
        let sx = cache.x_map[dx] as usize;
        if sx != run_sx {
            emit_run(
                dst,
                src,
                dst_row_base,
                src_row_base + run_sx * 4,
                run_start,
                dx,
            );
            run_sx = sx;
            run_start = dx;
        }
    }
    // Flush the trailing run. The row span always lands inside
    // `dst` because we bounds-checked `dst_row_base + dst_w_us * 4`
    // at the caller; the source span lands inside `src` for the same
    // reason on `src_row_base + src_w_us * 4`.
    emit_run(
        dst,
        src,
        dst_row_base,
        src_row_base + run_sx * 4,
        run_start,
        dst_w_us,
    );
}

/// Downscale: one source pixel per dst pixel. x_map[dx] gives the
/// source column directly. The x_map cache already computes this in
/// `StretchCache::ensure`, so the inner loop is just two reads + a
/// 4-byte store.
fn emit_row_downscale(
    dst: &mut [u8],
    src: &[u8],
    dst_row_base: usize,
    src_row_base: usize,
    cache: &StretchCache,
    dst_w_us: usize,
) {
    for dx in 0..dst_w_us {
        let src_off = src_row_base + cache.x_map[dx] as usize * 4;
        let dst_off = dst_row_base + dx * 4;
        // SAFETY: bounds checked at the caller. This is the only
        // 4-byte copy in the row that doesn't share the source with a
        // neighbour, so no way to batch it into a wider store.
        dst[dst_off..dst_off + 4].copy_from_slice(&src[src_off..src_off + 4]);
    }
}

/// Emit one Bresenham run: dst pixels `[run_start, run_end)` all
/// receive the same source pixel (read once). Caller bounds-checks
/// both source + destination row spans.
#[inline]
fn emit_run(
    dst: &mut [u8],
    src: &[u8],
    dst_row_base: usize,
    src_off: usize,
    run_start: usize,
    run_end: usize,
) {
    // Source pixel is constant within the run — lift the read once.
    let pixel = &src[src_off..src_off + 4];
    for dx in run_start..run_end {
        let dst_off = dst_row_base + dx * 4;
        dst[dst_off..dst_off + 4].copy_from_slice(pixel);
    }
}

#[cfg(test)]
mod bresenham_tests {
    use super::{StretchCache, stretch_byte_rows};

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
