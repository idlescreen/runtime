// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Aspect-preserving letterbox upscale with black bars.

use crate::FilterMode;

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
                filter,
            );
            write_pixel(dst, dst_w, out_x, out_y, color);
        }
    }
}
