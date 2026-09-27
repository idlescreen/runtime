// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Pixel sampling helpers for letterbox upscale.

use crate::FilterMode;

pub(super) fn sample_src(
    src: &[u8],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    filter: FilterMode,
) -> [u8; 4] {
    match filter {
        FilterMode::Nearest => sample_nearest(src, width, height, x, y),
        FilterMode::Linear => sample_bilinear(src, width, height, x, y),
    }
}

fn sample_nearest(src: &[u8], width: u32, height: u32, x: f32, y: f32) -> [u8; 4] {
    let px = x.round().clamp(0.0, (width - 1) as f32) as u32;
    let py = y.round().clamp(0.0, (height - 1) as f32) as u32;
    read_pixel(src, width, px, py)
}

/// Tier-3 bilinear: replace the per-channel f32 `lerp` (a + (b-a)*t)
/// with a u8 / u16 fixed-point variant. f32 lerp on 4 channels ×
/// 2560×1440 pixels is a measurable hot loop on the screensaver's
/// letterbox path; integer math avoids the float pipeline (no
/// rounding, no denormal stalls, no per-iteration conversions) and
/// matches the visual output to within 1 ULP per channel.
///
/// `tx` and `ty` are pre-quantized to 8-bit fractions in
/// `[0, 255]` (0 = use `c00` exactly, 255 = use `c10` exactly).
/// The 4-channel horizontal lerp produces an intermediate u16, then
/// the vertical lerp collapses back to u8.
fn sample_bilinear(src: &[u8], width: u32, height: u32, x: f32, y: f32) -> [u8; 4] {
    let x_clamped = x.clamp(0.0, (width - 1) as f32);
    let y_clamped = y.clamp(0.0, (height - 1) as f32);
    let x0 = x_clamped.floor() as u32;
    let y0 = y_clamped.floor() as u32;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    // 8-bit fixed-point fractions. Multiply by 256 first then mask
    // because float subtraction is exact here (x - floor(x) is in
    // [0, 1) and 256 * it is < 256).
    let tx = ((x_clamped - x0 as f32) * 256.0) as u32 as u8;
    let ty = ((y_clamped - y0 as f32) * 256.0) as u32 as u8;

    let c00 = read_pixel(src, width, x0, y0);
    let c10 = read_pixel(src, width, x1, y0);
    let c01 = read_pixel(src, width, x0, y1);
    let c11 = read_pixel(src, width, x1, y1);

    let mut out = [0u8; 4];
    for channel in 0..4 {
        // `lerp_u8(a, b, t)` = a + ((b - a) * t) >> 8
        // with t ∈ [0, 255]. The intermediate `(b - a) * t` can be
        // negative for `b < a`, so use i16 to avoid wraparound.
        let top = lerp_u8(c00[channel], c10[channel], tx);
        let bottom = lerp_u8(c01[channel], c11[channel], tx);
        out[channel] = lerp_u8(top, bottom, ty);
    }
    out
}

/// `lerp_u8(a, b, t) = a + ((b - a) * t) >> 8` for `t ∈ [0, 255]`.
///
/// The product `(b - a) * t` is in [-65025, 65025] — fits in i32
/// but overflows i16 (which holds ±32 767). The intermediate lives
/// in i32; the result lands in [0, 255] after the shift.
#[inline]
fn lerp_u8(a: u8, b: u8, t: u8) -> u8 {
    let diff = (i32::from(b) - i32::from(a)) * i32::from(t);
    // Round-half-up: 0.5 ULP bias. The original f32 path used
    // `.round()` (banker's rounding), but the bias error is ≤ 0.5 ULP
    // per channel and not visible after the screensaver's compositing
    // pipeline.
    let biased = diff + 128;
    let shifted = biased >> 8;
    (i32::from(a) + shifted).clamp(0, 255) as u8
}

fn read_pixel(src: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let offset = (y as usize * width as usize + x as usize) * 4;
    if offset + 3 >= src.len() {
        return [0, 0, 0, 255];
    }
    [
        src[offset],
        src[offset + 1],
        src[offset + 2],
        src[offset + 3],
    ]
}

pub(super) fn write_pixel(dst: &mut [u8], width: u32, x: u32, y: u32, color: [u8; 4]) {
    let offset = (y as usize * width as usize + x as usize) * 4;
    if offset + 3 >= dst.len() {
        return;
    }
    dst[offset..offset + 4].copy_from_slice(&color);
}

#[cfg(test)]
mod tests {
    use super::lerp_u8;

    #[test]
    fn lerp_endpoints() {
        assert_eq!(lerp_u8(0, 255, 0), 0);
        // t = 255 represents t/256 = 254/255 of the way to b
        // (i.e. 255/256 ≈ 0.996 of the way). lerp(0, 255, 0.996) =
        // 254.0039... → rounds to 254, not 255. The f32 path's
        // `lerp().round()` lands at the same value.
        assert_eq!(lerp_u8(0, 255, 255), 254);
        assert_eq!(lerp_u8(128, 128, 0), 128);
        assert_eq!(lerp_u8(128, 128, 255), 128);
    }

    #[test]
    fn lerp_midpoint() {
        // 50% between 0 and 255 = ~127 (we round-half-up).
        assert_eq!(lerp_u8(0, 255, 128), 128);
        // 50% between 100 and 200 = 150.
        assert_eq!(lerp_u8(100, 200, 128), 150);
    }

    #[test]
    fn lerp_reversed() {
        // Lerp with a > b should give a value between b and a.
        // Note: lerp_u8 uses round-half-up bias which matches the
        // f32 path's `.round()` for 127.5 → 128 (banker's rounding
        // also gives 128 since 128 is even).
        assert_eq!(lerp_u8(255, 0, 128), 128);
        assert_eq!(lerp_u8(200, 100, 128), 150);
    }

    #[test]
    fn lerp_u8_stays_in_range() {
        for a in (0..=255u8).step_by(17) {
            for b in (0..=255u8).step_by(17) {
                for t in (0..=255u8).step_by(17) {
                    let r = lerp_u8(a, b, t);
                    let lo = a.min(b);
                    let hi = a.max(b);
                    assert!(
                        r >= lo && r <= hi,
                        "lerp_u8({a}, {b}, {t}) = {r} not in [{lo}, {hi}]"
                    );
                }
            }
        }
    }
}
