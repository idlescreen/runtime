// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Aspect-preserving letterbox upscale with black bars.

use crate::FilterMode;

use super::bilinear_row::bilinear_row;
use super::sample::{sample_src, write_pixel};

#[allow(clippy::too_many_arguments)]
pub fn upscale_letterbox_into(
    dst: &mut [u8],
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    filter: FilterMode,
) {
    let needed = (dst_w * dst_h * 4) as usize;
    if dst.len() < needed {
        return;
    }
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        dst[..needed].fill(0);
        return;
    }

    // Tier-3 perf change: instead of zero-filling the entire destination
    // (a ~6 MB memset at 2560×1440 that the inner loop then immediately
    // overwrites), only fill the four black-bar regions around
    // `offset_x .. offset_x + display_w` × `offset_y .. offset_y + display_h`.
    // When the aspect ratio matches and there are no bars (common for
    // preview windows at the saver's native size), the fill cost is zero.
    let scale = (dst_w as f32 / src_w as f32).min(dst_h as f32 / src_h as f32);
    let display_w = (src_w as f32 * scale).floor() as u32;
    let display_h = (src_h as f32 * scale).floor() as u32;
    let offset_x = (dst_w - display_w) / 2;
    let offset_y = (dst_h - display_h) / 2;

    let row_bytes = (dst_w * 4) as usize;
    if offset_y > 0 {
        let top_rows = offset_y as usize;
        dst[..top_rows * row_bytes].fill(0);
    }
    if offset_x > 0 {
        let left_bytes = (offset_x * 4) as usize;
        let right_bytes = ((dst_w - offset_x - display_w) * 4) as usize;
        for dst_y in offset_y..(offset_y + display_h) {
            let row_start = dst_y as usize * row_bytes;
            dst[row_start..row_start + left_bytes].fill(0);
            if right_bytes > 0 {
                let right_start = row_start + row_bytes - right_bytes;
                dst[right_start..row_start + row_bytes].fill(0);
            }
        }
    } else if display_w < dst_w {
        let right_bytes = ((dst_w - display_w) * 4) as usize;
        for dst_y in offset_y..(offset_y + display_h) {
            let row_start = dst_y as usize * row_bytes;
            let right_start = row_start + row_bytes - right_bytes;
            dst[right_start..row_start + row_bytes].fill(0);
        }
    }
    if (offset_y + display_h) < dst_h {
        let bottom_start = ((offset_y + display_h) as usize) * row_bytes;
        dst[bottom_start..needed].fill(0);
    }

    // Tier-3 SIMD fast path: the Linear (bilinear) filter is the
    // dominant hot path. Process 4 horizontally-adjacent display
    // pixels per iteration via `bilinear_row` (SSE2 on x86_64,
    // scalar fallback elsewhere). The remainder (when `display_w`
    // is not a multiple of 4) falls through to `write_pixel`.
    match filter {
        FilterMode::Nearest => {
            for dst_y in 0..display_h {
                for dst_x in 0..display_w {
                    let out_x = offset_x + dst_x;
                    let out_y = offset_y + dst_y;
                    let color = sample_src(
                        src,
                        src_w,
                        src_h,
                        (dst_x as f32 + 0.5) / display_w as f32 * src_w as f32 - 0.5,
                        (dst_y as f32 + 0.5) / display_h as f32 * src_h as f32 - 0.5,
                        FilterMode::Nearest,
                    );
                    write_pixel(dst, dst_w, out_x, out_y, color);
                }
            }
        }
        FilterMode::Linear => {
            for dst_y in 0..display_h {
                // Compute the fractional y once per row — every
                // pixel in a row shares it.
                let fy = (dst_y as f32 + 0.5) / display_h as f32 * src_h as f32 - 0.5;
                let y_clamped = fy.clamp(0.0, (src_h - 1) as f32);
                let y0 = y_clamped.floor() as u32;
                let y1 = (y0 + 1).min(src_h - 1);
                let ty = ((y_clamped - y0 as f32) * 256.0) as u32 as u8;

                let out_y = offset_y + dst_y;
                let row_start = out_y as usize * row_bytes + offset_x as usize * 4;

                // Process the display area in chunks of 4 pixels.
                let chunks = display_w / 4;
                for chunk in 0..chunks {
                    let dst_x = chunk * 4;
                    let fx = (dst_x as f32 + 0.5) / display_w as f32 * src_w as f32 - 0.5;
                    let x0_clamped = fx.clamp(0.0, (src_w - 1) as f32);
                    let x0 = x0_clamped.floor() as u32;
                    let tx = ((x0_clamped - x0 as f32) * 256.0) as u32 as u8;
                    let x1 = (x0 + 1).min(src_w - 1);

                    let mut block = [0u8; 16];
                    bilinear_row(src, src_w, src_h, x0, y0, x1, y1, tx, ty, &mut block);

                    let col_start = row_start + dst_x as usize * 4;
                    dst[col_start..col_start + 16].copy_from_slice(&block);
                }

                // Tail (when display_w is not a multiple of 4) —
                // scalar `sample_src` for the leftover 1–3 pixels.
                for dst_x in (chunks * 4)..display_w {
                    let out_x = offset_x + dst_x;
                    let color = sample_src(
                        src,
                        src_w,
                        src_h,
                        (dst_x as f32 + 0.5) / display_w as f32 * src_w as f32 - 0.5,
                        (dst_y as f32 + 0.5) / display_h as f32 * src_h as f32 - 0.5,
                        FilterMode::Linear,
                    );
                    write_pixel(dst, dst_w, out_x, out_y, color);
                }
            }
        }
    }
}
