// SPDX-License-Identifier: MIT

const FONT_W: u32 = 8;
const _FONT_H: u32 = 8;

pub fn draw_top_right(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    label: &str,
    color: (u8, u8, u8),
) {
    if width == 0 || height == 0 || label.is_empty() {
        return;
    }

    let text_w = label.len() as u32 * FONT_W;
    let margin = 12u32;
    let origin_x = width.saturating_sub(text_w + margin);
    let origin_y = margin;

    for (idx, ch) in label.chars().enumerate() {
        let x = origin_x + idx as u32 * FONT_W;
        draw_char(pixels, width, height, x, origin_y, ch, color);
    }
}

fn draw_char(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    ch: char,
    color: (u8, u8, u8),
) {
    let glyph = glyph_rows(ch);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..FONT_W {
            if bits & (1 << (7 - col)) != 0 {
                put_pixel(pixels, width, height, x + col, y + row as u32, color);
            }
        }
    }
}

fn put_pixel(pixels: &mut [u8], width: u32, height: u32, x: u32, y: u32, rgb: (u8, u8, u8)) {
    if x >= width || y >= height {
        return;
    }
    let idx = ((y * width + x) * 4) as usize;
    if idx + 3 >= pixels.len() {
        return;
    }
    pixels[idx] = rgb.2;
    pixels[idx + 1] = rgb.1;
    pixels[idx + 2] = rgb.0;
    pixels[idx + 3] = 255;
}

fn glyph_rows(ch: char) -> [u8; 8] {
    match ch {
        '0'..='9' => digit_glyph(ch as u8 - b'0'),
        '.' => [0, 0, 0, 0, 0, 0, 0x18, 0x18],
        '/' => [0x06, 0x0c, 0x18, 0x30, 0x60, 0, 0, 0],
        '|' => [0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18],
        ' ' => [0; 8],
        'F' => [0x7e, 0x60, 0x7c, 0x60, 0x60, 0x60, 0x60, 0],
        'P' => [0x7c, 0x66, 0x66, 0x7c, 0x60, 0x60, 0x60, 0],
        'S' => [0x3c, 0x60, 0x30, 0x1c, 0x06, 0x06, 0x3c, 0],
        't' => [0x18, 0x7e, 0x18, 0x18, 0x18, 0x18, 0x0e, 0],
        'i' => [0x18, 0, 0x18, 0x18, 0x18, 0x18, 0x3c, 0],
        'c' => [0x3c, 0x60, 0x60, 0x60, 0x60, 0x60, 0x3c, 0],
        'k' => [0x60, 0x60, 0x60, 0x78, 0x6c, 0x66, 0x66, 0],
        _ => [0x7e, 0x81, 0x99, 0x89, 0x7e, 0, 0, 0],
    }
}

fn digit_glyph(digit: u8) -> [u8; 8] {
    match digit {
        0 => [0x3c, 0x66, 0x6e, 0x76, 0x66, 0x66, 0x3c, 0],
        1 => [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7e, 0],
        2 => [0x3c, 0x66, 0x06, 0x0c, 0x30, 0x60, 0x7e, 0],
        3 => [0x3c, 0x66, 0x06, 0x1c, 0x06, 0x66, 0x3c, 0],
        4 => [0x0c, 0x1c, 0x2c, 0x4c, 0x7e, 0x0c, 0x0c, 0],
        5 => [0x7e, 0x60, 0x7c, 0x06, 0x06, 0x66, 0x3c, 0],
        6 => [0x3c, 0x60, 0x60, 0x7c, 0x66, 0x66, 0x3c, 0],
        7 => [0x7e, 0x06, 0x0c, 0x18, 0x30, 0x30, 0x30, 0],
        8 => [0x3c, 0x66, 0x66, 0x3c, 0x66, 0x66, 0x3c, 0],
        9 => [0x3c, 0x66, 0x66, 0x3e, 0x06, 0x06, 0x3c, 0],
        _ => [0; 8],
    }
}
