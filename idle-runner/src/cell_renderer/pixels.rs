// SPDX-License-Identifier: MIT

#![allow(clippy::too_many_arguments)]

pub fn letterbox_into(
    content: &[u8],
    content_w: u32,
    content_h: u32,
    width: u32,
    height: u32,
    offset_x: usize,
    offset_y: usize,
    framed: &mut [u8],
) {
    let needed = (width as usize)
        .checked_mul(height as usize)
        .and_then(|p| p.checked_mul(4))
        .unwrap_or(0);
    if framed.len() < needed {
        return;
    }
    framed[..needed].fill(0);

    for row in 0..content_h as usize {
        let src_start = row * content_w as usize * 4;
        let src_end = src_start + content_w as usize * 4;
        let dst_row = offset_y + row;
        if dst_row >= height as usize {
            break;
        }
        let dst_start = (dst_row * width as usize + offset_x) * 4;
        let dst_end = dst_start + content_w as usize * 4;
        if src_end <= content.len() && dst_end <= framed.len() {
            framed[dst_start..dst_end].copy_from_slice(&content[src_start..src_end]);
        }
    }
}

pub fn fill_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    color: (u8, u8, u8),
) {
    let limit_y = y.saturating_add(h).min(height as usize);
    let limit_x = x.saturating_add(w).min(width as usize);
    if limit_x <= x || limit_y <= y {
        return;
    }

    let px_val = [color.2, color.1, color.0, 0xFF];

    for row in y..limit_y {
        let start_offset = (row * width as usize + x) * 4;
        let end_offset = (row * width as usize + limit_x) * 4;
        if end_offset <= pixels.len() {
            let row_slice = &mut pixels[start_offset..end_offset];
            for chunk in row_slice.as_chunks_mut::<4>().0 {
                *chunk = px_val;
            }
        }
    }
}

pub fn dim_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) {
    let limit_y = y.saturating_add(h).min(height as usize);
    let limit_x = x.saturating_add(w).min(width as usize);
    if limit_x <= x || limit_y <= y {
        return;
    }

    for row in y..limit_y {
        let row_start = (row * width as usize + x) * 4;
        let row_end = (row * width as usize + limit_x) * 4;
        if row_end <= pixels.len() {
            let row_slice = &mut pixels[row_start..row_end];
            let (prefix, u64_chunks, suffix) = unsafe { row_slice.align_to_mut::<u64>() };
            for p in prefix.iter_mut() {
                *p >>= 1;
            }
            for chunk in u64_chunks.iter_mut() {
                *chunk = (*chunk >> 1) & 0x7F7F7F7F_7F7F7F7F;
            }
            for s in suffix.iter_mut() {
                *s >>= 1;
            }
        }
    }
}

pub fn blit_bitmap(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: usize,
    y: usize,
    bitmap: &[u8],
    bitmap_w: usize,
    bitmap_h: usize,
    color: (u8, u8, u8),
) {
    for row in 0..bitmap_h {
        let py = y.saturating_add(row);
        if py >= height as usize {
            break;
        }
        let bitmap_row_start = row * bitmap_w;
        let row_pixel_start = (py * width as usize) * 4;

        for col in 0..bitmap_w {
            let px = x.saturating_add(col);
            if px >= width as usize {
                break;
            }
            let alpha = *bitmap.get(bitmap_row_start + col).unwrap_or(&0) as u32;
            if alpha == 0 {
                continue;
            }

            let offset = row_pixel_start + px * 4;
            if offset + 3 >= pixels.len() {
                continue;
            }

            if alpha == 255 {
                pixels[offset] = color.2;
                pixels[offset + 1] = color.1;
                pixels[offset + 2] = color.0;
                pixels[offset + 3] = 0xFF;
            } else {
                let inv_a = 255 - alpha;
                let dst_b = pixels[offset] as u32;
                let dst_g = pixels[offset + 1] as u32;
                let dst_r = pixels[offset + 2] as u32;

                pixels[offset] = ((color.2 as u32 * alpha + dst_b * inv_a) / 255) as u8;
                pixels[offset + 1] = ((color.1 as u32 * alpha + dst_g * inv_a) / 255) as u8;
                pixels[offset + 2] = ((color.0 as u32 * alpha + dst_r * inv_a) / 255) as u8;
                pixels[offset + 3] = 0xFF;
            }
        }
    }
}
