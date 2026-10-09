// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Host OS and Desktop Environment detection for branding and screensaver wordmarks.
//!
//! Evaluates active DE, host OS from `/etc/os-release`, or configured custom text.

/// Parses `NAME` and `PRETTY_NAME` from os-release content.
///
/// Prefers `NAME` when clean (e.g. "Fedora Linux", "Arch Linux", "Ubuntu"),
/// falling back to `PRETTY_NAME`.
pub fn parse_os_release(content: &str) -> Option<String> {
    let mut name = None;
    let mut pretty = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(v) = trimmed.strip_prefix("NAME=") {
            let s = v.trim().trim_matches('"').trim_matches('\'').trim();
            if !s.is_empty() {
                name = Some(s.to_string());
            }
        } else if let Some(v) = trimmed.strip_prefix("PRETTY_NAME=") {
            let s = v.trim().trim_matches('"').trim_matches('\'').trim();
            if !s.is_empty() {
                pretty = Some(s.to_string());
            }
        }
    }
    name.or(pretty)
}

/// Dynamically detects the Host OS by inspecting `/etc/os-release` and `/usr/lib/os-release`.
pub fn detect_host_os() -> Option<String> {
    for path in ["/etc/os-release", "/usr/lib/os-release"] {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Some(os) = parse_os_release(&text)
        {
            return Some(os);
        }
    }
    None
}

/// Normalizes raw desktop environment strings (from `XDG_CURRENT_DESKTOP`, etc.)
/// into canonical branding names (e.g. "COSMIC", "GNOME", "Hyprland").
pub fn normalize_desktop_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("cosmic") {
        Some("COSMIC".to_string())
    } else if lower.contains("hyprland") {
        Some("Hyprland".to_string())
    } else if lower.contains("sway") {
        Some("Sway".to_string())
    } else if lower.contains("gnome") {
        Some("GNOME".to_string())
    } else if lower.contains("kde") || lower.contains("plasma") {
        Some("KDE Plasma".to_string())
    } else if lower.contains("xfce") {
        Some("Xfce".to_string())
    } else {
        Some(trimmed.to_string())
    }
}

/// Dynamically detects the active desktop environment from session environment variables.
pub fn detect_desktop_environment() -> Option<String> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return Some("Hyprland".to_string());
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        return Some("Sway".to_string());
    }
    for key in [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ] {
        if let Ok(val) = std::env::var(key)
            && let Some(de) = normalize_desktop_name(&val)
        {
            return Some(de);
        }
    }
    None
}

/// Resolves the canonical branding / wordmark string.
///
/// Priority:
/// 1. User customization via `[saver] <saver_name>.text` or `text` (via saver param env vars).
/// 2. Active desktop environment (e.g. "COSMIC", "GNOME", "Hyprland").
/// 3. Host operating system (e.g. "Fedora Linux", "Arch Linux", "Ubuntu").
/// 4. Fallback product brand "IDLESCREEN".
pub fn detect_brand_text() -> String {
    for key in [
        "IDLE_SAVER_PARAM_ASCII_TEXT",
        "IDLE_SAVER_PARAM_BRAND_TEXT",
        "IDLE_SAVER_PARAM_TEXT",
        "IDLE_LOGO_TEXT",
    ] {
        if let Ok(v) = std::env::var(key) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    for (k, v) in std::env::vars() {
        if k.starts_with("IDLE_SAVER_PARAM_") && k.ends_with("_TEXT") {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    if let Some(de) = detect_desktop_environment() {
        return de;
    }

    if let Some(os) = detect_host_os() {
        return os;
    }

    "IDLESCREEN".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_os_release_extracts_name_first() {
        let fedora = "NAME=\"Fedora Linux\"\nVERSION=\"44 (Server Edition)\"\nPRETTY_NAME=\"Fedora Linux 44 (Server Edition)\"";
        assert_eq!(parse_os_release(fedora).as_deref(), Some("Fedora Linux"));

        let arch = "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"";
        assert_eq!(parse_os_release(arch).as_deref(), Some("Arch Linux"));

        let ubuntu = "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"";
        assert_eq!(parse_os_release(ubuntu).as_deref(), Some("Ubuntu"));
    }

    #[test]
    fn parse_os_release_falls_back_to_pretty_name() {
        let custom = "PRETTY_NAME=\"Custom Distro Linux\"";
        assert_eq!(
            parse_os_release(custom).as_deref(),
            Some("Custom Distro Linux")
        );
    }

    #[test]
    fn normalize_desktop_identifies_major_desktops() {
        assert_eq!(normalize_desktop_name("COSMIC").as_deref(), Some("COSMIC"));
        assert_eq!(normalize_desktop_name("cosmic").as_deref(), Some("COSMIC"));
        assert_eq!(normalize_desktop_name("GNOME").as_deref(), Some("GNOME"));
        assert_eq!(
            normalize_desktop_name("ubuntu:GNOME").as_deref(),
            Some("GNOME")
        );
        assert_eq!(
            normalize_desktop_name("Hyprland").as_deref(),
            Some("Hyprland")
        );
        assert_eq!(normalize_desktop_name("sway").as_deref(), Some("Sway"));
        assert_eq!(
            normalize_desktop_name("KDE:Plasma").as_deref(),
            Some("KDE Plasma")
        );
        assert_eq!(normalize_desktop_name("XFCE").as_deref(), Some("Xfce"));
        assert_eq!(normalize_desktop_name("   "), None);
    }

    #[test]
    fn detect_brand_text_returns_non_empty_string() {
        let brand = detect_brand_text();
        assert!(!brand.is_empty());
    }
}
