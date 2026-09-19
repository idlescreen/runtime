// SPDX-License-Identifier: MIT

#![allow(clippy::too_many_arguments)]

use std::sync::OnceLock;

use fontdue::Font;

const FONT_SIZE: f32 = 20.0;

/// Monospace TTF paths across Fedora/RHEL package layouts and Debian/Ubuntu.
/// Order prefers high-quality fixed-width faces already common on desktops.
const FONT_CANDIDATES: &[&str] = &[
    // Fedora / RHEL package dirs (dejavu-sans-mono-fonts, liberation-mono-fonts, …)
    "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
    "/usr/share/fonts/liberation-mono-fonts/LiberationMono-Regular.ttf",
    "/usr/share/fonts/adwaita-mono-fonts/AdwaitaMono-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSansMono-Regular.ttf",
    "/usr/share/fonts/google-noto-vf/NotoSansMono[wght].ttf",
    // Debian / Ubuntu
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
    // Flat / legacy multi-distro locations
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
];

static FONT: OnceLock<Option<Font>> = OnceLock::new();

pub fn init_font() {
    let _ = font();
}

/// Paths tried for the caption overlay (for tests / diagnostics).
pub fn caption_font_candidates() -> &'static [&'static str] {
    FONT_CANDIDATES
}

fn font() -> Option<&'static Font> {
    FONT.get_or_init(|| {
        for path in FONT_CANDIDATES {
            if let Ok(bytes) = std::fs::read(path)
                && let Ok(font) = Font::from_bytes(bytes, fontdue::FontSettings::default())
            {
                idle_log::info!("loaded caption font from {}", path);
                return Some(font);
            }
        }
        idle_log::warn!(
            "no caption font found (tried {} candidates); caption overlay disabled",
            FONT_CANDIDATES.len()
        );
        None
    })
    .as_ref()
}

/// Draw a readable bottom-centered caption bar at native monitor resolution.
pub fn draw_bottom_center(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    text: &str,
    fg: (u8, u8, u8),
) {
    let trimmed = text.trim();
    if trimmed.is_empty() || width == 0 || height == 0 {
        return;
    }

    let Some(font) = font() else {
        return;
    };

    let (metrics, _) = font.rasterize('M', FONT_SIZE);
    let line_h = metrics.height.max(14) as u32;
    let bar_pad_x = 18u32;
    let bar_pad_y = 10u32;
    let bottom_margin = 28u32;

    let mut text_w = 0u32;
    for ch in trimmed.chars() {
        let (metrics, _) = font.rasterize(ch, FONT_SIZE);
        text_w += metrics.advance_width.max(1.0).ceil() as u32;
    }

    let bar_w = (text_w + bar_pad_x * 2).min(width.saturating_sub(24));
    let bar_h = line_h + bar_pad_y * 2;
    let bar_x = width.saturating_sub(bar_w) / 2;
    let bar_y = height.saturating_sub(bar_h + bottom_margin);

    fill_rect(
        pixels,
        width,
        height,
        bar_x,
        bar_y,
        bar_w,
        bar_h,
        (12, 14, 20),
        210,
    );

    let mut pen_x = bar_x + bar_pad_x;
    let pen_y = bar_y + bar_pad_y;
    for ch in trimmed.chars() {
        let (metrics, bitmap) = font.rasterize(ch, FONT_SIZE);
        blit_glyph(
            pixels,
            width,
            height,
            pen_x,
            pen_y,
            metrics.width,
            metrics.height,
            &bitmap,
            fg,
        );
        pen_x += metrics.advance_width.max(1.0).ceil() as u32;
        if pen_x >= bar_x + bar_w.saturating_sub(bar_pad_x) {
            break;
        }
    }
}

fn fill_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    rgb: (u8, u8, u8),
    alpha: u8,
) {
    for row in y..y.saturating_add(h).min(height) {
        for col in x..x.saturating_add(w).min(width) {
            blend_pixel(pixels, width, col, row, rgb, alpha);
        }
    }
}

fn blit_glyph(
    pixels: &mut [u8],
    width: u32,
    _height: u32,
    x: u32,
    y: u32,
    glyph_w: usize,
    glyph_h: usize,
    bitmap: &[u8],
    fg: (u8, u8, u8),
) {
    for row in 0..glyph_h {
        for col in 0..glyph_w {
            let idx = row * glyph_w + col;
            if idx >= bitmap.len() {
                continue;
            }
            let alpha = bitmap[idx];
            if alpha == 0 {
                continue;
            }
            blend_pixel(pixels, width, x + col as u32, y + row as u32, fg, alpha);
        }
    }
}

fn blend_pixel(pixels: &mut [u8], width: u32, x: u32, y: u32, rgb: (u8, u8, u8), alpha: u8) {
    if x >= width {
        return;
    }
    let idx = ((y * width + x) * 4) as usize;
    if idx + 3 >= pixels.len() {
        return;
    }
    let a = alpha as f32 / 255.0;
    let inv = 1.0 - a;
    pixels[idx] = (pixels[idx] as f32 * inv + rgb.2 as f32 * a) as u8;
    pixels[idx + 1] = (pixels[idx + 1] as f32 * inv + rgb.1 as f32 * a) as u8;
    pixels[idx + 2] = (pixels[idx + 2] as f32 * inv + rgb.0 as f32 * a) as u8;
    pixels[idx + 3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_candidates_include_fedora_and_debian_layouts() {
        let c = caption_font_candidates();
        assert!(c.iter().any(|p| p.contains("dejavu-sans-mono-fonts")));
        assert!(c.iter().any(|p| p.contains("truetype/dejavu")));
        assert!(c.iter().any(|p| p.contains("LiberationMono")));
    }

    #[test]
    fn font_init_succeeds_when_system_mono_present() {
        // On CI without fonts this may be None; on a desktop with DejaVu it loads.
        init_font();
        let any_present = FONT_CANDIDATES
            .iter()
            .any(|p| std::path::Path::new(p).is_file());
        if any_present {
            assert!(
                font().is_some(),
                "expected a caption font when a candidate file exists on disk"
            );
        }
    }
}
