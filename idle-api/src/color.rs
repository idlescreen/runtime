/// Convert HSL to RGB. `h` is in degrees [0, 360); `s`, `l` in [0.0, 1.0];
/// returns `(r, g, b)` with each channel in `[0, 255]`.
///
/// # Example
///
/// ```
/// use idle_api::hsl_to_rgb;
/// assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), (255, 0, 0));   // pure red
/// assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), (0, 255, 0)); // pure green
/// assert_eq!(hsl_to_rgb(240.0, 1.0, 0.5), (0, 0, 255)); // pure blue
/// ```
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r_prime, g_prime, b_prime) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r_prime + m) * 255.0).clamp(0.0, 255.0) as u8,
        ((g_prime + m) * 255.0).clamp(0.0, 255.0) as u8,
        ((b_prime + m) * 255.0).clamp(0.0, 255.0) as u8,
    )
}

/// RGB→HSL conversion.
pub fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let l = (max + min) / 2.0;
    let mut h = 0.0;
    let mut s = 0.0;
    if d > 0.0001 {
        s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        if max == r {
            h = (g - b) / d + (if g < b { 6.0 } else { 0.0 });
        } else if max == g {
            h = (b - r) / d + 2.0;
        } else {
            h = (r - g) / d + 4.0;
        }
        h *= 60.0;
    }
    (h, s, l)
}

/// Calculate percentage from two unsigned integers. Returns 0.0 if total is 0.
pub fn percentage(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f32 / total as f32) * 100.0
    }
}

/// Linear interpolation between two values. Factor clamped to [0, 1].
pub fn lerp(a: f32, b: f32, factor: f32) -> f32 {
    let clamped_factor = factor.clamp(0.0, 1.0);
    a + (b - a) * clamped_factor
}

pub(crate) fn dim_color(color: (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    (
        (color.0 as f32 * factor) as u8,
        (color.1 as f32 * factor) as u8,
        (color.2 as f32 * factor) as u8,
    )
}

pub(crate) fn hue_rotated(
    color: (u8, u8, u8),
    delta_deg: f32,
    target_lightness: f32,
) -> (u8, u8, u8) {
    let (h, _s, _l) = rgb_to_hsl(color.0, color.1, color.2);
    let new_h = (h + delta_deg).rem_euclid(360.0);
    hsl_to_rgb(new_h, 0.95, target_lightness)
}

mod theme;
pub use theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_red_at_zero() {
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), (255, 0, 0));
    }

    #[test]
    fn hsl_green_at_120() {
        assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), (0, 255, 0));
    }

    #[test]
    fn hsl_blue_at_240() {
        assert_eq!(hsl_to_rgb(240.0, 1.0, 0.5), (0, 0, 255));
    }

    #[test]
    fn hsl_grey_at_zero_saturation() {
        // Half-grey: HSL(0, 0, 0.5) -> RGB(127, 127, 127) after rounding
        assert_eq!(hsl_to_rgb(0.0, 0.0, 0.5), (127, 127, 127));
    }

    #[test]
    fn lerp_clamps_below_zero() {
        assert_eq!(lerp(10.0, 20.0, -0.5), 10.0);
    }

    #[test]
    fn lerp_clamps_above_one() {
        assert_eq!(lerp(10.0, 20.0, 1.5), 20.0);
    }

    #[test]
    fn lerp_midpoint() {
        assert_eq!(lerp(0.0, 100.0, 0.5), 50.0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;

    /// xorshift64* deterministic stand-in for proptest's generators.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        /// Uniform f32 in [lo, hi).
        fn f32(&mut self, lo: f32, hi: f32) -> f32 {
            let u = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
            lo + u * (hi - lo)
        }
        fn u64(&mut self, lo: u64, hi: u64) -> u64 {
            lo + self.next() % (hi - lo + 1)
        }
    }

    /// Clamped `lerp` lands on the near endpoint when factor is outside [0, 1],
    /// and otherwise lies between the endpoints (with float slack).
    #[test]
    fn lerp_respects_clamp() {
        let mut rng = Rng(0xC010_2001);
        for _ in 0..256 {
            let a = rng.f32(-1e3, 1e3);
            let b = rng.f32(-1e3, 1e3);
            let f = rng.f32(-10.0, 10.0);
            let v = lerp(a, b, f);
            let lo = a.min(b);
            let hi = a.max(b);
            let slack = 1e-2 * (1.0 + lo.abs().max(hi.abs()));
            assert!(v + slack >= lo && v <= hi + slack, "lerp({a},{b},{f})={v}");
        }
    }

    /// Zero total always yields 0.0 percentage.
    #[test]
    fn percentage_zero_total_is_zero() {
        let mut rng = Rng(0xC010_2002);
        for _ in 0..256 {
            assert_eq!(percentage(rng.next(), 0), 0.0);
        }
    }

    /// Percentage is in [0, 100] when used <= total and total > 0.
    #[test]
    fn percentage_in_unit_range() {
        let mut rng = Rng(0xC010_2003);
        for _ in 0..256 {
            let total = rng.u64(1, 1_000_000);
            let used = rng.u64(0, total);
            let p = percentage(used, total);
            assert!((0.0..=100.0).contains(&p), "p={p}");
        }
    }

    /// HSL→RGB→HSL preserves lightness well for vivid mid-tones (8-bit RGB
    /// quantization makes hue/sat brittle at extremes).
    #[test]
    fn hsl_rgb_lightness_stable() {
        let mut rng = Rng(0xC010_2004);
        for _ in 0..256 {
            let h = rng.f32(0.0, 360.0);
            let s = rng.f32(0.4, 1.0);
            let l = rng.f32(0.2, 0.8);
            let (r, g, b) = hsl_to_rgb(h, s, l);
            let (_h2, _s2, l2) = rgb_to_hsl(r, g, b);
            assert!(
                (l - l2).abs() <= 0.02,
                "lightness HSL({h},{s},{l}) -> RGB({r},{g},{b}) -> L={l2}"
            );
        }
    }

    /// Pure greys keep equal RGB channels for any lightness.
    #[test]
    fn greyscale_equal_channels() {
        let mut rng = Rng(0xC010_2005);
        for _ in 0..256 {
            let l = rng.f32(0.0, 1.0);
            let (r, g, b) = hsl_to_rgb(0.0, 0.0, l);
            assert_eq!(r, g);
            assert_eq!(g, b);
        }
    }
}
