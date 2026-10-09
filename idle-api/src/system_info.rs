/// Cross-platform "where are we running" descriptor.
#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub os: String,
    pub logo_text: String,
    pub kernel: String,
    pub hostname: String,
    pub cpu: String,
    pub uptime_secs: u64,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub mem_used_pct: f32,
    pub cpu_usage_pct: f32,
    pub power_status: String,
    pub disk_summary: String,
    pub gpus: String,
    pub monitors: String,
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
        if let Ok(val) = std::env::var(key) {
            let lower = val.to_ascii_lowercase();
            if lower.contains("cosmic") {
                return Some("COSMIC".to_string());
            } else if lower.contains("hyprland") {
                return Some("Hyprland".to_string());
            } else if lower.contains("sway") {
                return Some("Sway".to_string());
            } else if lower.contains("gnome") {
                return Some("GNOME".to_string());
            } else if lower.contains("kde") || lower.contains("plasma") {
                return Some("KDE Plasma".to_string());
            } else if lower.contains("xfce") {
                return Some("Xfce".to_string());
            } else if lower.contains("cinnamon") {
                return Some("Cinnamon".to_string());
            } else if lower.contains("mate") {
                return Some("MATE".to_string());
            } else if lower.contains("lxqt") {
                return Some("LXQt".to_string());
            } else {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}

/// Parses `NAME` and `PRETTY_NAME` from os-release content.
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
    if let Some(name) = crate::env_var_first(&["IDLE_OS_NAME"]) {
        return Some(name);
    }
    for path in ["/etc/os-release", "/usr/lib/os-release"] {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Some(os) = parse_os_release(&text)
        {
            return Some(os);
        }
    }
    None
}

impl Default for SystemInfo {
    fn default() -> Self {
        if Self::export_mode_enabled() {
            return Self::export_fixture();
        }
        let os = detect_host_os().unwrap_or_else(|| "Linux".to_string());

        // The shared wordmark every saver renders.
        //
        // One source of truth: the host's OS name, overridable with
        // IDLE_LOGO_TEXT. Savers used to derive this independently and drift —
        // eleven read it from here while ascii hardcoded its own string, so
        // one machine showed different words on different savers.
        //
        // The fallback matters for the browser demos, where there is no
        // /etc/os-release and every saver would otherwise render the bare
        // string "Linux". Falling back to the product name keeps every demo
        // identical to every other.
        let logo_text = crate::env_var_first(&[
            "IDLE_SAVER_PARAM_ASCII_TEXT",
            "IDLE_SAVER_PARAM_BRAND_TEXT",
            "IDLE_SAVER_PARAM_TEXT",
            "IDLE_LOGO_TEXT",
        ])
        .or_else(detect_desktop_environment)
        .or_else(detect_host_os)
        .unwrap_or_else(|| "IDLESCREEN".into());

        let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());

        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        Self {
            os,
            logo_text,
            kernel,
            hostname,
            cpu: "CPU".to_string(),
            uptime_secs: 0,
            mem_used_mb: 1,
            mem_total_mb: 2,
            mem_used_pct: 50.0,
            cpu_usage_pct: 0.0,
            power_status: "AC".to_string(),
            disk_summary: "disks".to_string(),
            gpus: "GPU".to_string(),
            monitors: "1 monitor".to_string(),
        }
    }
}

impl SystemInfo {
    /// Stable fixture for offline export (`IDLE_EXPORT_MODE=1` / render).
    pub fn export_fixture() -> Self {
        Self {
            os: "IdleScreen Export".into(),
            logo_text: "IdleScreen".into(),
            kernel: "export".into(),
            hostname: "export-host".into(),
            cpu: "CPU".into(),
            uptime_secs: 0,
            mem_used_mb: 1024,
            mem_total_mb: 2048,
            mem_used_pct: 50.0,
            cpu_usage_pct: 0.0,
            power_status: "AC".into(),
            disk_summary: "disk".into(),
            gpus: "GPU".into(),
            monitors: "1 monitor".into(),
        }
    }

    /// True when host should prefer deterministic export fixtures.
    pub fn export_mode_enabled() -> bool {
        crate::env_truthy(&["IDLE_EXPORT_MODE"])
            || std::env::var_os("RENDER_SEED").is_some()
            || std::env::var_os("IDLE_RENDER_SEED").is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_os_release_prefers_name() {
        let text = "NAME=\"Fedora Linux\"\nVERSION=\"41\"\nPRETTY_NAME=\"Fedora Linux 41\"";
        assert_eq!(parse_os_release(text).as_deref(), Some("Fedora Linux"));
    }

    #[test]
    fn parse_os_release_falls_back_to_pretty_name() {
        let text = "PRETTY_NAME='Arch Linux'\nID=arch";
        assert_eq!(parse_os_release(text).as_deref(), Some("Arch Linux"));
    }

    #[test]
    fn parse_os_release_empty_returns_none() {
        assert_eq!(parse_os_release(""), None);
        assert_eq!(parse_os_release("FOO=bar\nBAZ=qux"), None);
    }

    #[test]
    fn detect_desktop_environment_and_host_os_smoke() {
        let _ = detect_desktop_environment();
        let _ = detect_host_os();
    }
}
