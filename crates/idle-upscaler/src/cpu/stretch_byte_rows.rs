// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Unaligned-byte nearest-neighbor stretch.
//!
//! Used when `src` or `dst` aren't 4-byte aligned (the u32 fast path
//! rejected them). Bresenham-span emit on upscale: each run of
//! consecutive dst pixels that share a source pixel reads the source
//! once. Downscale falls back to per-pixel copy (every dst pixel gets
//! a distinct source pixel).

use super::stretch_cache::StretchCache;

/// Per-row nearest-neighbor stretch on byte slices.
///
/// `_needed` is kept in the signature so the call site stays uniform
/// across the aligned (`stretch_u32_rows`) and unaligned byte paths;
/// was previously used by `dst[..needed].fill(0)` before that memset
/// was dropped.
#[allow(clippy::too_many_arguments)]
pub fn stretch_byte_rows(
    dst: &mut [u8],
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
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

// Tests + bench live in `stretch_byte_rows_tests.rs` so this page
// stays under the 256-line cap (the three Bresenham tests + bench
// block alone are ~150 lines).
#[cfg(test)]
#[path = "stretch_byte_rows_tests.rs"]
mod tests;