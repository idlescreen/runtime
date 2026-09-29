use crate::color::{dim_color, hue_rotated};

/// The canonical apps 4.0 screen palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPalette {
    pub bg: (u8, u8, u8),
    pub fg: (u8, u8, u8),
    pub accent: (u8, u8, u8),
    pub dim: (u8, u8, u8),
    pub hot: (u8, u8, u8),
    pub cool: (u8, u8, u8),
    pub mid: (u8, u8, u8),
    pub peak: (u8, u8, u8),
}

impl Default for ScreenPalette {
    fn default() -> Self {
        Self::from_system((46, 204, 113), true)
    }
}

impl ScreenPalette {
    pub fn from_system(accent: (u8, u8, u8), is_dark_mode: bool) -> Self {
        if is_dark_mode {
            Self {
                bg: (0, 0, 0),
                fg: (248, 248, 242),
                accent,
                dim: dim_color(accent, 0.35),
                hot: hue_rotated(accent, 30.0, 0.55),
                cool: hue_rotated(accent, -120.0, 0.45),
                mid: (128, 128, 128),
                peak: (255, 255, 255),
            }
        } else {
            Self {
                bg: (252, 252, 250),
                fg: (40, 42, 54),
                accent,
                dim: dim_color(accent, 0.7),
                hot: hue_rotated(accent, 30.0, 0.55),
                cool: hue_rotated(accent, -120.0, 0.45),
                mid: (160, 160, 160),
                peak: (255, 255, 255),
            }
        }
    }

    pub fn high_contrast(accent: (u8, u8, u8), is_dark_mode: bool) -> Self {
        let mut p = Self::from_system(accent, is_dark_mode);
        if is_dark_mode {
            p.bg = (0, 0, 0);
            p.fg = (255, 255, 255);
        } else {
            p.bg = (255, 255, 255);
            p.fg = (0, 0, 0);
        }
        p
    }

    pub fn default_dark() -> Self {
        Self::from_system((0, 245, 255), true)
    }

    pub fn default_light() -> Self {
        Self::from_system((0, 180, 200), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(rgb: (u8, u8, u8)) -> u32 {
        (rgb.0 as u32) + (rgb.1 as u32) + (rgb.2 as u32)
    }

    #[test]
    fn default_dark_has_dark_bg() {
        let p = ScreenPalette::default_dark();
        assert!(sum(p.bg) < 128 * 3);
    }

    #[test]
    fn default_light_has_light_bg() {
        let p = ScreenPalette::default_light();
        assert!(sum(p.bg) > 128 * 3);
    }

    #[test]
    fn default_dark_and_light_have_dark_and_light_fg() {
        let dark = ScreenPalette::default_dark();
        let light = ScreenPalette::default_light();
        assert!(sum(dark.fg) > sum(dark.bg));
        assert!(sum(light.fg) < sum(light.bg));
    }

    #[test]
    fn high_contrast_dark_polarizes() {
        let p = ScreenPalette::high_contrast((0, 200, 100), true);
        assert_eq!(p.bg, (0, 0, 0));
        assert_eq!(p.fg, (255, 255, 255));
    }

    #[test]
    fn high_contrast_light_polarizes() {
        let p = ScreenPalette::high_contrast((0, 200, 100), false);
        assert_eq!(p.bg, (255, 255, 255));
        assert_eq!(p.fg, (0, 0, 0));
    }

    #[test]
    fn high_contrast_polarizes_extremes() {
        let p = ScreenPalette::high_contrast((128, 128, 128), true);
        let sum_fg = sum(p.fg);
        let sum_bg = sum(p.bg);
        assert!(sum_fg.abs_diff(sum_bg) > 200);
    }

    #[test]
    fn palette_default_uses_dark_mode() {
        let p = ScreenPalette::default();
        assert_eq!(p.bg, (0, 0, 0));
    }
}
