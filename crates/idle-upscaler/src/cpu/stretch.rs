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
    needed: usize,
    cache: &StretchCache,
) {
    // Fallback unaligned byte-copy path
    dst[..needed].fill(0);
    for dy in 0..dst_h {
        let sy = (dy as u64 * src_h as u64 / dst_h as u64) as u32;
        let src_row = sy as usize * src_w as usize * 4;
        let dst_row = dy as usize * dst_w as usize * 4;
        for dx in 0..dst_w as usize {
            let src_off = src_row + cache.x_map[dx] as usize * 4;
            let dst_off = dst_row + dx * 4;
            if src_off + 4 <= src.len() && dst_off + 4 <= dst.len() {
                dst[dst_off..dst_off + 4].copy_from_slice(&src[src_off..src_off + 4]);
            }
        }
    }
}
