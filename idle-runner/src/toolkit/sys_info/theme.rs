// SPDX-License-Identifier: MIT

use std::sync::OnceLock;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::toolkit::theme_query::load_global_theme;

#[cfg(target_os = "linux")]
use super::linux_proc;

static DARK_MODE_CACHE: OnceLock<RwLock<(Option<bool>, Instant)>> = OnceLock::new();

/// Detect dark mode preference. Cached for 3 seconds.
pub fn query_dark_mode() -> bool {
    let cache_rw = DARK_MODE_CACHE.get_or_init(|| RwLock::new((None, Instant::now())));
    if let Ok(read_guard) = cache_rw.read()
        && let Some(val) = read_guard.0
        && read_guard.1.elapsed() < Duration::from_secs(3)
    {
        return val;
    }
    let mut cache = cache_rw.write().unwrap_or_else(|e| {
        idle_log::error!("mutex poisoned: {e}");
        std::process::abort()
    });
    if let Some(val) = cache.0
        && cache.1.elapsed() < Duration::from_secs(3)
    {
        return val;
    }
    let val = query_dark_mode_raw();
    cache.0 = Some(val);
    cache.1 = Instant::now();
    val
}

fn query_dark_mode_raw() -> bool {
    if let (_, Some(dark)) = load_global_theme() {
        return dark;
    }
    linux_proc::query_dark_mode_linux()
}

/// Query the system palette from accent + dark mode.
pub fn query_current_palette() -> crate::core::screen_palette::ScreenPalette {
    let (global_accent, global_dark) = load_global_theme();
    let dark = global_dark.unwrap_or_else(query_dark_mode);
    let accent = global_accent.unwrap_or((0, 245, 255));
    crate::core::screen_palette::ScreenPalette::from_system(accent, dark)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTheme {
    pub is_dark_mode: bool,
    pub is_high_contrast: bool,
    pub accent_color: (u8, u8, u8),
}

pub fn query_system_theme() -> SystemTheme {
    let (global_accent, global_dark) = load_global_theme();
    let dark = global_dark.unwrap_or_else(query_dark_mode);
    let accent = global_accent.unwrap_or((0, 245, 255));
    SystemTheme {
        is_dark_mode: dark,
        is_high_contrast: false,
        accent_color: accent,
    }
}
