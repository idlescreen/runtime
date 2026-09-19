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

    dst[..needed].fill(0);

    let scale = (dst_w as f32 / src_w as f32).min(dst_h as f32 / src_h as f32);
    let display_w = (src_w as f32 * scale).floor() as u32;
    let display_h = (src_h as f32 * scale).floor() as u32;
    let offset_x = (dst_w - display_w) / 2;
    let offset_y = (dst_h - display_h) / 2;

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
