//! Theme loader and parsing queries.
//! Linux-only (Windows accent/dark detection removed).

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Prefer IdleScreen config (`idle`), then legacy `trance`.
fn get_global_theme_path() -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| std::path::PathBuf::from(home).join(".config"))
        })?;
    let idle = base.join("idle").join("config.yaml");
    if idle.is_file() {
        return Some(idle);
    }
    let trance = base.join("trance").join("config.yaml");
    if trance.is_file() {
        return Some(trance);
    }
    // Default write/read target for new installs.
    Some(idle)
}
type ThemeSettings = (Option<(u8, u8, u8)>, Option<bool>);
type CacheEntry = (Option<ThemeSettings>, Instant);

static GLOBAL_THEME_CACHE: OnceLock<Mutex<CacheEntry>> = OnceLock::new();

pub fn load_global_theme() -> (Option<(u8, u8, u8)>, Option<bool>) {
    let cache_mutex = GLOBAL_THEME_CACHE.get_or_init(|| Mutex::new((None, Instant::now())));
    let mut cache = cache_mutex.lock().unwrap_or_else(|e| {
        idle_log::error!("theme cache mutex poisoned: {e}");
        std::process::abort()
    });
    if let Some(ref val) = cache.0
        && cache.1.elapsed() < Duration::from_secs(1)
    {
        return *val;
    }
    let val = load_global_theme_raw();
    cache.0 = Some(val);
    cache.1 = Instant::now();
    val
}

fn load_global_theme_raw() -> (Option<(u8, u8, u8)>, Option<bool>) {
    if let Some(path) = get_global_theme_path()
        && let Ok(content) = std::fs::read_to_string(path)
    {
        let mut accent = None;
        let mut dark = None;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(idx) = line.find(':') {
                let key = line[..idx].trim();
                let val = line[idx + 1..].trim().trim_matches('"').trim_matches('\'');
                match key {
                    "accent_color" => {
                        if !val.is_empty()
                            && val != "none"
                            && val.starts_with('#')
                            && val.len() == 7
                            && let (Ok(r), Ok(g), Ok(b)) = (
                                u8::from_str_radix(&val[1..3], 16),
                                u8::from_str_radix(&val[3..5], 16),
                                u8::from_str_radix(&val[5..7], 16),
                            )
                        {
                            accent = Some((r, g, b));
                        }
                    }
                    "dark_mode" | "is_dark_mode" => {
                        if let Ok(b) = val.parse::<bool>() {
                            dark = Some(b);
                        }
                    }
                    _ => {}
                }
            }
        }
        return (accent, dark);
    }
    (None, None)
}
